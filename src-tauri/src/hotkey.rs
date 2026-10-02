// VoiceKey – hotkey.rs
// Registers and manages the configurable global hold-to-talk shortcut.
//
// Uses tauri-plugin-global-shortcut for cross-platform registration.
// Key-down starts a session; key-up finalizes it.

use crate::app_state::{SessionState, SharedAppState};
use crate::transcription::{
    TranscriptionCoordinator, EVENT_ERROR, EVENT_STATE,
};
use crate::{audio, errors::VoiceKeyError, insertion, settings::Settings, transcription::StateEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tokio::sync::oneshot;
use tracing::{error, info, warn};

/// Register the global hold-to-talk shortcut.
///
/// On key-down: start audio capture + transcription coordinator.
/// On key-up: signal the coordinator to finalize, then insert text.
pub fn register_shortcut(app: &AppHandle, settings: &Settings) -> Result<(), VoiceKeyError> {
    let shortcut: Shortcut = settings.shortcut.parse().map_err(|_| {
        VoiceKeyError::HotkeyRegistrationFailed(settings.shortcut.clone())
    })?;

    let app_handle = app.clone();

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            let state: tauri::State<SharedAppState> = app.state();
            let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();

            match event.state() {
                ShortcutState::Pressed => {
                    on_key_down(app.clone(), state.inner().clone(), settings_state.inner().clone());
                }
                ShortcutState::Released => {
                    on_key_up(app.clone(), state.inner().clone(), settings_state.inner().clone());
                }
            }
        })
        .map_err(|e| VoiceKeyError::HotkeyRegistrationFailed(e.to_string()))?;

    info!("Global shortcut registered: {}", settings.shortcut);
    Ok(())
}

/// Called on hotkey press.  Starts a new session unless already listening.
fn on_key_down(app: AppHandle, state: SharedAppState, settings: parking_lot::RwLock<Settings>) {
    // Ignore repeated key-down events while already in progress.
    {
        let s = state.read();
        if !matches!(s.current_state(), SessionState::Idle) {
            return;
        }
    }

    let session_id = state.write().start_listening();
    info!("Session {session_id}: LISTENING");

    let _ = app.emit(
        EVENT_STATE,
        StateEvent {
            session_id: session_id.clone(),
            state: "LISTENING".into(),
        },
    );

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
                crate::transcription::ErrorEvent {
                    session_id,
                    code: code.into(),
                    message: e.to_string(),
                },
            );
            return;
        }
    };

    // A oneshot channel lets us signal the coordinator to stop (value = true for cancel).
    let (stop_tx, stop_rx) = oneshot::channel::<bool>();

    // Store the stop sender in Tauri's managed state so on_key_up can retrieve it.
    // We use a Mutex<Option<Sender>> pattern.
    {
        let stop_cell: tauri::State<parking_lot::Mutex<Option<oneshot::Sender<bool>>>> =
            app.state();
        *stop_cell.lock() = Some(stop_tx);
    }

    // Spawn the coordinator on a Tokio task so it doesn't block the hotkey callback.
    let settings_guard = settings.read();
    let model_filename = settings_guard.model.filename().to_string();
    let language = settings_guard.language.clone();
    drop(settings_guard);

    let app_clone = app.clone();
    let state_clone = state.clone();
    let session_id_clone = session_id.clone();
    let max_secs = {
        let s = settings.read();
        s.max_recording_secs
    };

    tokio::spawn(async move {
        // Resolve the model path.
        let model_path = match crate::transcription::model_path(&app_clone, &model_filename) {
            Ok(p) => p,
            Err(e) => {
                error!("Model not found: {e}");
                state_clone.write().set_error(&session_id_clone, e.to_string());
                let _ = app_clone.emit(
                    EVENT_ERROR,
                    crate::transcription::ErrorEvent {
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

        // If max_recording_secs > 0, auto-cancel after that duration.
        if max_secs > 0 {
            let timeout_cell: tauri::State<parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<bool>>>> =
                app_clone.state();
            let app_t = app_clone.clone();
            let state_t = state_clone.clone();
            let sid_t = session_id_clone.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(max_secs as u64)).await;
                // Only fire if still listening.
                if state_t.read().is_listening() {
                    info!("Max recording duration ({max_secs}s) reached — auto-finalizing");
                    let mut cell = timeout_cell.lock();
                    if let Some(tx) = cell.take() {
                        let _ = tx.send(false); // false = normal release, not cancel
                    }
                    let _ = app_t.emit(
                        crate::transcription::EVENT_STATE,
                        StateEvent {
                            session_id: sid_t,
                            state: "FINALIZING".into(),
                        },
                    );
                }
            });
        }

        coordinator.run(audio_rx, session_id_clone.clone(), stop_rx).await;


        // After coordinator finishes, attempt text insertion if in INSERTING state.
        let final_text = {
            let s = state_clone.read();
            if let Some(ref sess) = s.session {
                if sess.id == session_id_clone {
                    sess.final_text.clone()
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(text) = final_text {
            let _ = app_clone.emit(
                EVENT_STATE,
                StateEvent {
                    session_id: session_id_clone.clone(),
                    state: "INSERTING".into(),
                },
            );

            let use_fallback = {
                let settings_guard: tauri::State<parking_lot::RwLock<Settings>> = app_clone.state();
                settings_guard.read().clipboard_fallback
            };

            match insertion::insert_text(&text, use_fallback).await {
                Ok(_) => {
                    info!("Text inserted successfully");
                }
                Err(e) => {
                    error!("Insertion failed: {e}");
                    let _ = app_clone.emit(
                        EVENT_ERROR,
                        crate::transcription::ErrorEvent {
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
            StateEvent {
                session_id: session_id_clone,
                state: "IDLE".into(),
            },
        );

        audio_handle.stop();
    });
}

/// Called on hotkey release.  Signals the coordinator to finalize.
fn on_key_up(app: AppHandle, state: SharedAppState, _settings: parking_lot::RwLock<Settings>) {
    if !matches!(state.read().current_state(), SessionState::Listening) {
        return;
    }

    // Send stop signal (false = normal release, not cancel).
    let stop_cell: tauri::State<parking_lot::Mutex<Option<oneshot::Sender<bool>>>> = app.state();
    if let Some(tx) = stop_cell.lock().take() {
        let _ = tx.send(false);
    }
}
