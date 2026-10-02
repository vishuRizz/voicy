// VoiceKey – hotkey.rs
// Registers and manages the configurable global hold-to-talk shortcut.

use crate::app_state::{SessionState, SharedAppState};
use crate::transcription::{
    ErrorEvent, StateEvent, TranscriptionCoordinator, EVENT_ERROR, EVENT_STATE,
};
use crate::{audio, errors::VoiceKeyError, insertion, settings::Settings, transcription};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::oneshot;
use tracing::{error, info};

/// Register the global hold-to-talk shortcut.
///
/// `tauri-plugin-global-shortcut` uses the same key names as the browser
/// `KeyboardEvent.code` spec.  macOS Option key = `Alt`.  So the default
/// shortcut string must be `"Alt+Space"`, NOT `"Option+Space"`.
pub fn register_shortcut(app: &AppHandle, settings: &Settings) -> Result<(), VoiceKeyError> {
    // Normalise: accept "Option+" as an alias so existing persisted settings work.
    let normalised = settings.shortcut
        .replace("Option+", "Alt+")
        .replace("option+", "Alt+");

    let shortcut: Shortcut = normalised.parse().map_err(|e| {
        VoiceKeyError::HotkeyRegistrationFailed(format!("{normalised}: {e}"))
    })?;

    info!("Registering global shortcut: {} (raw: {})", normalised, settings.shortcut);

    let worker = hotkey_worker();
    // Carbon hotkeys often deliver the press but not the release once another
    // app is focused. Watch the physical key-up so hold-to-talk can end.
    let release_worker = worker.clone();
    let release_app = app.clone();
    crate::platform::install_release_watch(move || {
        let _ = release_worker.send(HotkeyCmd::Up(release_app.clone()));
    });

    app.global_shortcut()
        .on_shortcut(shortcut, move |app, shortcut, event| {
            info!("Shortcut event: {:?} state={:?}", shortcut, event.state());
            // The plugin invokes this from a Carbon event handler (extern "C").
            // A panic here aborts the process. Only enqueue; do the work on the worker.
            let cmd = match event.state() {
                ShortcutState::Pressed => HotkeyCmd::Down(app.clone()),
                ShortcutState::Released => HotkeyCmd::Up(app.clone()),
            };
            if worker.send(cmd).is_err() {
                error!("Hotkey worker is gone — shortcut ignored");
            }
        })
        .map_err(|e| VoiceKeyError::HotkeyRegistrationFailed(e.to_string()))?;

    info!("Global shortcut registered OK: {}", normalised);
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
    let (model_filename, language, max_secs, use_fallback, shortcut) = {
        let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
        let s = settings_state.read();
        (
            s.model.filename().to_string(),
            s.language.clone(),
            s.max_recording_secs,
            s.clipboard_fallback,
            s.shortcut.clone(),
        )
    };
    crate::platform::arm_release_watch(&shortcut);

    // Start audio capture.
    let (audio_handle, audio_rx) = match audio::start_capture() {
        Ok(pair) => pair,
        Err(e) => {
            crate::platform::clear_release_watch();
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

    // Not a Tokio thread (hotkey worker). `tokio::spawn` panics here.
    tauri::async_runtime::spawn(async move {
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
            tauri::async_runtime::spawn(async move {
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
    signal_stop(&app, &state);
}

/// End the hold. Safe to call twice (Carbon release and the physical key-up
/// monitor both arrive). The first call wins.
fn signal_stop(app: &AppHandle, state: &SharedAppState) {
    if !matches!(state.read().current_state(), SessionState::Listening) {
        return;
    }
    crate::platform::clear_release_watch();

    let session_id = state.read().current_session_id().unwrap_or_default();
    state.write().begin_finalizing(&session_id);
    info!("Key released — finalizing session {session_id}");
    let _ = app.emit(
        EVENT_STATE,
        StateEvent { session_id, state: "FINALIZING".into() },
    );

    let tx = {
        let stop_cell: tauri::State<std::sync::Arc<parking_lot::Mutex<Option<oneshot::Sender<bool>>>>> =
            app.state();
        let mut guard = stop_cell.lock();
        guard.take()
    };
    if let Some(tx) = tx {
        let _ = tx.send(false);
    } else {
        error!("Key released but no stop signal was armed");
    }
}

// ── Worker ────────────────────────────────────────────────────────────────────
//
// global-hotkey delivers press/release from an `extern "C"` Carbon callback
// inside `NSApplication::sendEvent`. Anything that panics there aborts the
// process (`panic_cannot_unwind`). The callback only enqueues; this thread
// runs press and release in order.

enum HotkeyCmd {
    Down(AppHandle),
    Up(AppHandle),
}

fn hotkey_worker() -> mpsc::Sender<HotkeyCmd> {
    static TX: OnceLock<mpsc::Sender<HotkeyCmd>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<HotkeyCmd>();
        std::thread::Builder::new()
            .name("voicekey-hotkey".into())
            .spawn(move || {
                while let Ok(cmd) = rx.recv() {
                    let app_for_reset = match &cmd {
                        HotkeyCmd::Down(app) | HotkeyCmd::Up(app) => app.clone(),
                    };
                    let panicked = catch_unwind(AssertUnwindSafe(|| match cmd {
                        HotkeyCmd::Down(app) => {
                            let state = {
                                let guard: tauri::State<SharedAppState> = app.state();
                                guard.inner().clone()
                            };
                            on_key_down(app, state);
                        }
                        HotkeyCmd::Up(app) => {
                            let state = {
                                let guard: tauri::State<SharedAppState> = app.state();
                                guard.inner().clone()
                            };
                            on_key_up(app, state);
                        }
                    }));
                    if panicked.is_err() {
                        error!("Hotkey handler panicked — resetting session");
                        let state: tauri::State<SharedAppState> = app_for_reset.state();
                        state.write().finish();
                    }
                }
            })
            .expect("failed to spawn hotkey worker");
        tx
    })
    .clone()
}
