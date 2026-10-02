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
    app_state::SharedAppState,
    audio::AudioReceiver,
    audio::TARGET_SAMPLE_RATE,
    errors::VoiceKeyError,
};
use anyhow::Result;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
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

    // Suppress blank outputs. Timestamps must be off: on a short utterance
    // whisper.cpp logs "single timestamp ending - skip entire chunk" and
    // returns no text even though it decoded the words.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_timestamps(true);
    params.set_single_segment(true);
    params.set_no_context(true);
    params.set_suppress_blank(true);

    // whisper.cpp drops or mishandles clips shorter than ~1s.
    let mut padded = samples.to_vec();
    let min_samples = TARGET_SAMPLE_RATE as usize;
    if padded.len() < min_samples {
        padded.resize(min_samples, 0.0);
    }

    state
        .full(params, &padded)
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

    let n = state
        .full_n_segments()
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

        let text = (0..n)
        .filter_map(|i| state.full_get_segment_text(i).ok())
        .collect::<Vec<_>>()
        .join(" ");

    let text = text.trim().to_string();
    info!("Transcript: {text:?}");
    Ok(text)
}

// ── TranscriptionCoordinator ─────────────────────────────────────────────────

/// Manages both the rolling-window preview loop and the final-pass inference.
pub struct TranscriptionCoordinator {
    app: AppHandle,
    state: SharedAppState,
    model_path: PathBuf,
    language: String,
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
            preview_window: (preview_window_secs * TARGET_SAMPLE_RATE) as usize,
            preview_interval: Duration::from_secs(preview_interval_secs),
        }
    }

    async fn transcribe(&self, samples: Vec<f32>) -> Result<String> {
        if samples.len() < (TARGET_SAMPLE_RATE / 4) as usize {
            return Ok(String::new());
        }
        let model_path = self.model_path.clone();
        let language = self.language.clone();
        tauri::async_runtime::spawn_blocking(move || run_whisper(&samples, &model_path, &language))
            .await
            .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?
    }

    /// Run inference on the most recent preview window and emit a preview event.
    async fn emit_preview(&self, session_id: &str, window: Vec<f32>) {
        match self.transcribe(window).await {
            Ok(text) if text.is_empty() => {}
            Ok(text) => {
                if !self.state.read().is_listening() {
                    return;
                }
                debug!("Preview: {text}");
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
    async fn emit_final(&self, session_id: &str, samples: Vec<f32>) -> Result<String> {
        info!(
            "Final pass: {} samples ({:.1} s)",
            samples.len(),
            samples.len() as f32 / TARGET_SAMPLE_RATE as f32
        );

        let text = self.transcribe(samples).await?;
        if text.is_empty() {
            return Err(anyhow::anyhow!(
                "ASR_INFERENCE_FAILED: no speech detected"
            ));
        }

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
        self,
        rx: AudioReceiver,
        session_id: String,
        mut stop_rx: oneshot::Receiver<bool>, // true = cancel
    ) {
        // Drain the mic on its own task so a slow Whisper pass cannot fill
        // the audio channel and drop the utterance.
        let pcm = Arc::new(parking_lot::Mutex::new(Vec::<f32>::new()));
        let pcm_ingest = pcm.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                match rx.recv_async().await {
                    Ok(chunk) => pcm_ingest.lock().extend(chunk),
                    Err(_) => break,
                }
            }
        });

        let mut ticker = interval(self.preview_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            // `biased` so a queued key-up always wins over another preview.
            // A preview also must not keep running after release: that is
            // what left the UI stuck on Listening.
            select! {
                biased;
                result = &mut stop_rx => {
                    self.finish(result, &session_id, &pcm).await;
                    return;
                }
                _ = ticker.tick() => {
                    if !self.state.read().is_listening() {
                        continue;
                    }
                    let window = {
                        let buf = pcm.lock();
                        let start = buf.len().saturating_sub(self.preview_window);
                        buf[start..].to_vec()
                    };
                    if window.len() < TARGET_SAMPLE_RATE as usize / 2 {
                        continue;
                    }
                    let preview = self.emit_preview(&session_id, window);
                    tokio::pin!(preview);
                    select! {
                        biased;
                        result = &mut stop_rx => {
                            drop(preview);
                            self.finish(result, &session_id, &pcm).await;
                            return;
                        }
                        _ = &mut preview => {}
                    }
                }
            }
        }
    }

    async fn finish(
        &self,
        result: std::result::Result<bool, tokio::sync::oneshot::error::RecvError>,
        session_id: &str,
        pcm: &Arc<parking_lot::Mutex<Vec<f32>>>,
    ) {
        // Let the ingest task pull chunks already queued.
        tokio::time::sleep(Duration::from_millis(40)).await;
        let cancelled = result.unwrap_or(true);

        if cancelled {
            info!("Session {session_id} cancelled");
            self.state.write().cancel();
            let _ = self.app.emit(
                EVENT_STATE,
                StateEvent {
                    session_id: session_id.to_string(),
                    state: "IDLE".into(),
                },
            );
            return;
        }

        if self.state.read().is_listening() {
            self.state.write().begin_finalizing(session_id);
        }
        let _ = self.app.emit(
            EVENT_STATE,
            StateEvent {
                session_id: session_id.to_string(),
                state: "FINALIZING".into(),
            },
        );

        let samples = pcm.lock().clone();
        match self.emit_final(session_id, samples).await {
            Ok(_) => {}
            Err(e) => {
                error!("Final inference failed: {e}");
                self.state.write().set_error(session_id, e.to_string());
                let _ = self.app.emit(
                    EVENT_ERROR,
                    ErrorEvent {
                        session_id: session_id.to_string(),
                        code: "ASR_INFERENCE_FAILED".into(),
                        message: e.to_string(),
                    },
                );
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
