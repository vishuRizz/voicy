// VoiceKey – hotkey.rs
// Registers and manages the configurable global hold-to-talk shortcut.

use crate::app_state::{SessionState, SharedAppState};
use crate::transcription::{
    ErrorEvent, StateEvent, TranscriptionCoordinator, EVENT_ERROR, EVENT_STATE,
};
use crate::{audio, errors::VoiceKeyError, insertion, settings::Settings, transcription};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::oneshot;
use tracing::{error, info};

/// Register the global hold-to-talk shortcut.
pub fn register_shortcut(app: &AppHandle, settings: &Settings) -> Result<(), VoiceKeyError> {
    let shortcut: Shortcut = settings.shortcut.parse().map_err(|_| {
        VoiceKeyError::HotkeyRegistrationFailed(settings.shortcut.clone())
    })?;

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            let state: tauri::State<SharedAppState> = app.state();
            match event.state() {
                ShortcutState::Pressed => on_key_down(app.clone(), state.inner().clone()),
                ShortcutState::Released => on_key_up(app.clone(), state.inner().clone()),
            }
        })
        .map_err(|e| VoiceKeyError::HotkeyRegistrationFailed(e.to_string()))?;

    info!("Global shortcut registered: {}", settings.shortcut);
    Ok(())
}

// ── Key-down ─────────────────────────────────────────────────────────────────

fn on_key_down(app: AppHandle, state: SharedAppState) {
    // Ignore repeated key-down while already in progress.
    if !matches!(state.read().current_state(), SessionState::Idle) {
        return;
    }

    let session_id = state.write().start_listening();
    info!("Session {session_id}: LISTENING");

    let _ = app.emit(
        EVENT_STATE,
        StateEvent { session_id: session_id.clone(), state: "LISTENING".into() },
    );

    // Read settings from managed state.
    let (model_filename, language, max_secs, use_fallback) = {
        let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
        let s = settings_state.read();
        (
            s.model.filename().to_string(),
            s.language.clone(),
            s.max_recording_secs,
            s.clipboard_fallback,
        )
    };

    // Start audio capture.
    let (audio_handle, audio_rx) = match audio::start_capture() {
        Ok(pair) => pair,
        Err(e) => {
            error!("Audio capture failed: {e}");
            let code = if e.to_string().contains("PERMISSION") {
                "MIC_PERMISSION_DENIED"
            } else {
                "MIC_DEVICE_UNAVAILABLE"
            };
            state.write().set_error(&session_id, e.to_string());
            let _ = app.emit(
                EVENT_ERROR,
                ErrorEvent {
                    session_id,
                    code: code.into(),
                    message: e.to_string(),
                },
            );
            return;
        }
    };

    // Oneshot for key-up / cancel signal.
    let (stop_tx, stop_rx) = oneshot::channel::<bool>();
    {
        let stop_cell: tauri::State<std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>> =
            app.state();
        *stop_cell.lock() = Some(stop_tx);
    }

    let app_clone = app.clone();
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();

    // Extract stop_arc BEFORE the spawn so we don't borrow app_clone inside it.
    let stop_arc_for_timer: Option<std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>> = if max_secs > 0 {
        let s: tauri::State<std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>> = app.state();
        Some(std::sync::Arc::clone(s.inner()))
    } else {
        None
    };

    tokio::spawn(async move {
        // Resolve model path.
        let model_path = match transcription::model_path(&app_clone, &model_filename) {
            Ok(p) => p,
            Err(e) => {
                error!("Model not found: {e}");
                state_clone.write().set_error(&session_id_clone, e.to_string());
                let _ = app_clone.emit(
                    EVENT_ERROR,
                    ErrorEvent {
                        session_id: session_id_clone.clone(),
                        code: "ASR_MODEL_MISSING".into(),
                        message: e.to_string(),
                    },
                );
                audio_handle.stop();
                return;
            }
        };

        let coordinator = TranscriptionCoordinator::new(
            app_clone.clone(),
            state_clone.clone(),
            model_path,
            language,
        );

        // Optional max-recording-duration auto-finalize.
        if let Some(stop_arc) = stop_arc_for_timer {
            let state_t = state_clone.clone();
            let sid_t = session_id_clone.clone();
            let app_for_timer = app_clone.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(max_secs as u64)).await;
                if state_t.read().is_listening() {
                    info!("Max recording duration ({max_secs}s) — auto-finalizing");
                    if let Some(tx) = stop_arc.lock().take() {
                        let _ = tx.send(false);
                    }
                    let _ = app_for_timer.emit(
                        EVENT_STATE,
                        StateEvent { session_id: sid_t, state: "FINALIZING".into() },
                    );
                }
            });
        }


        coordinator.run(audio_rx, session_id_clone.clone(), stop_rx).await;

        // Attempt text insertion if in INSERTING state.
        let final_text = {
            let s = state_clone.read();
            s.session.as_ref().and_then(|sess| {
                if sess.id == session_id_clone {
                    sess.final_text.clone()
                } else {
                    None
                }
            })
        };

        if let Some(text) = final_text {
            let _ = app_clone.emit(
                EVENT_STATE,
                StateEvent { session_id: session_id_clone.clone(), state: "INSERTING".into() },
            );

            match insertion::insert_text(&text, use_fallback).await {
                Ok(_) => info!("Text inserted successfully"),
                Err(e) => {
                    error!("Insertion failed: {e}");
                    let _ = app_clone.emit(
                        EVENT_ERROR,
                        ErrorEvent {
                            session_id: session_id_clone.clone(),
                            code: "INSERTION_FAILED".into(),
                            message: e.to_string(),
                        },
                    );
                }
            }
        }

        state_clone.write().finish();
        let _ = app_clone.emit(
            EVENT_STATE,
            StateEvent { session_id: session_id_clone, state: "IDLE".into() },
        );
        audio_handle.stop();
    });
}

// ── Key-up ────────────────────────────────────────────────────────────────────

fn on_key_up(app: AppHandle, state: SharedAppState) {
    if !matches!(state.read().current_state(), SessionState::Listening) {
        return;
    }
    // Use a named binding so the MutexGuard drops before end of block.
    let tx = {
        let stop_cell: tauri::State<std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>> =
            app.state();
        let mut guard = stop_cell.lock();
        let x = guard.take();
        x
    };
    if let Some(tx) = tx {
        let _ = tx.send(false);
    }
}
