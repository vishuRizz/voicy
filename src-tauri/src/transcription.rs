// VoiceKey – transcription.rs
// Manages rolling-window preview inference and full-utterance final inference
// via whisper.cpp (linked via the `whisper-rs` crate or a direct FFI stub).
//
// TRD §15 strategy:
//  1. Accumulate audio during the active session.
//  2. Every ~1–2 s, run inference on a recent ~4–6 s window → emit preview.
//  3. On key-release, run final pass on the complete buffer → emit final.
//  4. Never insert provisional text automatically.
//  5. Stale results identified by session_id are silently dropped.

use crate::{
    app_state::{SessionState, SharedAppState},
    audio::{AudioChunk, AudioReceiver, TARGET_SAMPLE_RATE},
    errors::VoiceKeyError,
};
use anyhow::Result;
use flume::Sender;
use std::{path::PathBuf, time::Duration};
use tauri::{AppHandle, Emitter};
use tokio::{select, sync::oneshot, time::interval};
use tracing::{debug, error, info, warn};

// ── public event names ───────────────────────────────────────────────────────

pub const EVENT_STATE: &str = "session://state";
pub const EVENT_PREVIEW: &str = "session://preview";
pub const EVENT_FINAL: &str = "session://final";
pub const EVENT_ERROR: &str = "session://error";

// ── event payloads ───────────────────────────────────────────────────────────

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct StateEvent {
    pub session_id: String,
    pub state: String,
}

#[derive(Serialize, Clone)]
pub struct PreviewEvent {
    pub session_id: String,
    pub text: String,
    pub is_provisional: bool,
}

#[derive(Serialize, Clone)]
pub struct FinalEvent {
    pub session_id: String,
    pub text: String,
}

#[derive(Serialize, Clone)]
pub struct ErrorEvent {
    pub session_id: String,
    pub code: String,
    pub message: String,
}

// ── model path helper ────────────────────────────────────────────────────────

/// Returns the path to the whisper model file, relative to the app's resource
/// directory.  Adjust if using a user-downloadable model directory instead.
pub fn model_path(app: &AppHandle, filename: &str) -> Result<PathBuf, VoiceKeyError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|_| VoiceKeyError::ModelMissing(filename.to_string()))?;
    let path = resource_dir.join("models").join(filename);
    if !path.exists() {
        return Err(VoiceKeyError::ModelMissing(filename.to_string()));
    }
    Ok(path)
}

// ── whisper wrapper (stub until whisper-rs is linked) ───────────────────────

/// A minimal wrapper around whisper.cpp.  In production replace the body with
/// real `whisper_rs` calls; the function signature is the contract.
///
/// # Arguments
/// * `samples` – f32 PCM at 16 kHz, mono
/// * `model_path` – path to the `.bin` model file
/// * `language` – BCP-47 code or "auto"
pub fn run_whisper(samples: &[f32], model_path: &PathBuf, language: &str) -> Result<String> {
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    let model_str = model_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("ASR_MODEL_MISSING: invalid model path"))?;

    // Load the model (whisper-rs caches the context internally on repeated calls
    // to the same path in the same process; a persistent-context optimisation
    // can be added once the session coordinator owns the WhisperContext).
    let ctx = WhisperContext::new_with_params(model_str, WhisperContextParameters::default())
        .map_err(|e| anyhow::anyhow!("ASR_MODEL_MISSING: {e}"))?;

    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });

    // Language: "auto" maps to None (auto-detection); everything else is a BCP-47 code.
    if language != "auto" {
        params.set_language(Some(language));
    }

    // Suppress blank outputs and excessive timestamps in the transcript.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);

    // whisper-rs expects exactly 16 kHz mono f32 — already guaranteed by audio.rs.
    state
        .full(params, samples)
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

    let n = state
        .full_n_segments()
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

    let text = (0..n)
        .filter_map(|i| state.full_get_segment_text(i).ok())
        .collect::<Vec<_>>()
        .join(" ");

    Ok(text.trim().to_string())
}

// ── TranscriptionCoordinator ─────────────────────────────────────────────────

/// Manages both the rolling-window preview loop and the final-pass inference.
pub struct TranscriptionCoordinator {
    app: AppHandle,
    state: SharedAppState,
    model_path: PathBuf,
    language: String,
    /// Rolling audio accumulator (all f32 samples from the session).
    pcm_buffer: Vec<f32>,
    /// Context window for preview inference (samples).
    preview_window: usize,
    /// How often to refresh the preview.
    preview_interval: Duration,
}

impl TranscriptionCoordinator {
    pub fn new(
        app: AppHandle,
        state: SharedAppState,
        model_path: PathBuf,
        language: String,
    ) -> Self {
        let preview_window_secs = 5u32; // §15: ~4–6 s
        let preview_interval_secs = 1u64; // §15: ~1–2 s

        TranscriptionCoordinator {
            app,
            state,
            model_path,
            language,
            pcm_buffer: Vec::new(),
            preview_window: (preview_window_secs * TARGET_SAMPLE_RATE) as usize,
            preview_interval: Duration::from_secs(preview_interval_secs),
        }
    }

    /// Drain incoming audio chunks into the local buffer.
    fn ingest(&mut self, rx: &AudioReceiver) {
        while let Ok(chunk) = rx.try_recv() {
            self.pcm_buffer.extend(chunk);
        }
    }

    /// Run inference on the most recent preview window and emit a preview event.
    fn emit_preview(&self, session_id: &str) {
        let start = self.pcm_buffer.len().saturating_sub(self.preview_window);
        let window = &self.pcm_buffer[start..];

        match run_whisper(window, &self.model_path, &self.language) {
            Ok(text) => {
                debug!("Preview: {text}");
                // Update state machine
                self.state
                    .write()
                    .update_preview(session_id, text.clone());

                let _ = self.app.emit(
                    EVENT_PREVIEW,
                    PreviewEvent {
                        session_id: session_id.to_string(),
                        text,
                        is_provisional: true,
                    },
                );
            }
            Err(e) => {
                warn!("Preview inference error: {e}");
            }
        }
    }

    /// Run the final full-utterance pass and emit the final event.
    fn emit_final(&self, session_id: &str) -> Result<String> {
        info!(
            "Final pass: {} samples ({:.1} s)",
            self.pcm_buffer.len(),
            self.pcm_buffer.len() as f32 / TARGET_SAMPLE_RATE as f32
        );

        let text = run_whisper(&self.pcm_buffer, &self.model_path, &self.language)?;

        self.state
            .write()
            .set_final_text(session_id, text.clone());

        let _ = self.app.emit(
            EVENT_FINAL,
            FinalEvent {
                session_id: session_id.to_string(),
                text: text.clone(),
            },
        );

        Ok(text)
    }

    /// Main loop.  Runs until the stop sender fires (key-up) or cancel fires.
    ///
    /// * `rx` – live audio receiver
    /// * `session_id` – session identifier for staleness checks
    /// * `stop_rx` – signals key-up; next value indicates cancel (true) or
    ///               normal release (false)
    pub async fn run(
        mut self,
        rx: AudioReceiver,
        session_id: String,
        mut stop_rx: oneshot::Receiver<bool>, // true = cancel
    ) {
        let mut ticker = interval(self.preview_interval);

        loop {
            select! {
                _ = ticker.tick() => {
                    self.ingest(&rx);
                    // Only emit preview if still in LISTENING state.
                    if self.state.read().is_listening() {
                        self.emit_preview(&session_id);
                    }
                }
                result = &mut stop_rx => {
                    let cancelled = result.unwrap_or(true);
                    self.ingest(&rx); // drain remaining

                    if cancelled {
                        info!("Session {session_id} cancelled");
                        self.state.write().cancel();
                        let _ = self.app.emit(
                            EVENT_STATE,
                            StateEvent {
                                session_id: session_id.clone(),
                                state: "IDLE".into(),
                            },
                        );
                        return;
                    }

                    // Key released — run final inference.
                    self.state.write().begin_finalizing(&session_id);
                    let _ = self.app.emit(
                        EVENT_STATE,
                        StateEvent {
                            session_id: session_id.clone(),
                            state: "FINALIZING".into(),
                        },
                    );

                    match self.emit_final(&session_id) {
                        Ok(_) => {
                            // insertion.rs picks up from here via the INSERTING state
                        }
                        Err(e) => {
                            error!("Final inference failed: {e}");
                            self.state
                                .write()
                                .set_error(&session_id, e.to_string());
                            let _ = self.app.emit(
                                EVENT_ERROR,
                                ErrorEvent {
                                    session_id,
                                    code: "ASR_INFERENCE_FAILED".into(),
                                    message: e.to_string(),
                                },
                            );
                        }
                    }
                    return;
                }
            }
        }
    }
}

// ── Tauri commands ───────────────────────────────────────────────────────────

/// Cancel the active recording session.
#[tauri::command]
pub async fn cancel_session(
    state: tauri::State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<(), String> {
    let session_id = state.read().current_session_id();
    if let Some(id) = session_id {
        state.write().cancel();
        let _ = app.emit(
            EVENT_STATE,
            StateEvent {
                session_id: id,
                state: "IDLE".into(),
            },
        );
    }
    Ok(())
}
