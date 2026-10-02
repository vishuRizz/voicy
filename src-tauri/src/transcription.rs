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
    insertion::LiveDraft,
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
fn load_whisper(model_path: &PathBuf) -> Result<whisper_rs::WhisperContext> {
    use whisper_rs::{WhisperContext, WhisperContextParameters};

    let model_str = model_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("ASR_MODEL_MISSING: invalid model path"))?;
    WhisperContext::new_with_params(model_str, WhisperContextParameters::default())
        .map_err(|e| anyhow::anyhow!("ASR_MODEL_MISSING: {e}"))
}

fn transcribe_with(
    ctx: &whisper_rs::WhisperContext,
    samples: &[f32],
    language: &str,
) -> Result<String> {
    // Multiple segments so a pause does not end the utterance. `no_timestamps`
    // makes whisper.cpp jump to the end of the chunk at the first end-of-text
    // token, which dropped everything after the opening phrase.
    let text = decode(ctx, samples, language, false)?;
    if !text.is_empty() {
        return Ok(text);
    }
    // Very short clips sometimes produce no segments when timestamps are on.
    decode(ctx, samples, language, true)
}

fn decode(
    ctx: &whisper_rs::WhisperContext,
    samples: &[f32],
    language: &str,
    no_timestamps: bool,
) -> Result<String> {
    use whisper_rs::{FullParams, SamplingStrategy};

    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}"))?;

    // best_of -1 keeps whisper.cpp's own default.
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: -1 });

    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_timestamps(no_timestamps);
    params.set_single_segment(false);
    params.set_no_context(true);
    params.set_suppress_blank(true);

    let whisper_lang = match language {
        "hi" | "hinglish" => "hi",
        other => other,
    };
    if whisper_lang != "auto" {
        params.set_language(Some(whisper_lang));
    }
    // English-only models never learned Hindi. Hinglish uses the multilingual
    // model with language "hi", then Devanagari is spelled in Latin. Do not
    // bias the English models with a Hinglish prompt; that made both worse.

    // whisper.cpp skips clips shorter than about a second.
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

    Ok(romanize_hindi(&text.split_whitespace().collect::<Vec<_>>().join(" ")))
}

/// If Whisper returns Devanagari, spell it in Latin. Latin text is unchanged.
fn romanize_hindi(text: &str) -> String {
    if !text.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)) {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut drop_final_a = false;
    while i < chars.len() {
        let c = chars[i];
        if let Some(mut base) = consonant_base(c) {
            i += 1;
            if i < chars.len() && chars[i] == '\u{093C}' {
                base = nukta_base(base);
                i += 1;
            }
            if i < chars.len() && chars[i] == '\u{094D}' {
                out.push_str(base);
                drop_final_a = false;
                i += 1;
                continue;
            }
            let mut vowel = "a";
            if i < chars.len() {
                if let Some(v) = matra(chars[i]) {
                    vowel = v;
                    i += 1;
                }
            }
            if vowel == "aa" && i < chars.len() && (chars[i] == '\u{0908}' || chars[i] == '\u{0907}') {
                vowel = "ai";
                i += 1;
            }
            out.push_str(base);
            out.push_str(vowel);
            drop_final_a = vowel == "a";
            continue;
        }
        if c == ' ' || c == '\n' || c == '\t' || c == '?' || c == '!' || c == ',' || c == '.' || c == '।' {
            if drop_final_a && out.ends_with('a') {
                out.pop();
            }
            drop_final_a = false;
        }
        if let Some(v) = independent_vowel(c) {
            out.push_str(v);
            drop_final_a = false;
            i += 1;
            continue;
        }
        match c {
            '\u{0902}' | '\u{0901}' => out.push('n'),
            '\u{0964}' => out.push('.'),
            '\u{0965}' => out.push_str(".."),
            _ => out.push(c),
        }
        if !('\u{0900}'..='\u{097F}').contains(&c) {
            drop_final_a = false;
        }
        i += 1;
    }
    if drop_final_a && out.ends_with('a') {
        out.pop();
    }
    out.replace("kyaa", "kya")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn consonant_base(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{0915}' => "k",
        '\u{0916}' => "kh",
        '\u{0917}' => "g",
        '\u{0918}' => "gh",
        '\u{0919}' => "ng",
        '\u{091A}' => "ch",
        '\u{091B}' => "chh",
        '\u{091C}' => "j",
        '\u{091D}' => "jh",
        '\u{091E}' => "ny",
        '\u{091F}' => "t",
        '\u{0920}' => "th",
        '\u{0921}' => "d",
        '\u{0922}' => "dh",
        '\u{0923}' => "n",
        '\u{0924}' => "t",
        '\u{0925}' => "th",
        '\u{0926}' => "d",
        '\u{0927}' => "dh",
        '\u{0928}' => "n",
        '\u{092A}' => "p",
        '\u{092B}' => "ph",
        '\u{092C}' => "b",
        '\u{092D}' => "bh",
        '\u{092E}' => "m",
        '\u{092F}' => "y",
        '\u{0930}' => "r",
        '\u{0932}' => "l",
        '\u{0935}' => "v",
        '\u{0936}' => "sh",
        '\u{0937}' => "sh",
        '\u{0938}' => "s",
        '\u{0939}' => "h",
        _ => return None,
    })
}

fn nukta_base(base: &str) -> &'static str {
    match base {
        "k" => "q",
        "kh" => "kh",
        "g" => "gh",
        "j" => "z",
        "ph" => "f",
        _ => "k",
    }
}

fn matra(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{093E}' => "aa",
        '\u{093F}' => "i",
        '\u{0940}' => "ee",
        '\u{0941}' => "u",
        '\u{0942}' => "oo",
        '\u{0943}' => "ri",
        '\u{0947}' => "e",
        '\u{0948}' => "ai",
        '\u{094B}' => "o",
        '\u{094C}' => "au",
        _ => return None,
    })
}

fn independent_vowel(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{0905}' => "a",
        '\u{0906}' => "aa",
        '\u{0907}' => "i",
        '\u{0908}' => "ee",
        '\u{0909}' => "u",
        '\u{090A}' => "oo",
        '\u{090F}' => "e",
        '\u{0910}' => "ai",
        '\u{0913}' => "o",
        '\u{0914}' => "au",
        _ => return None,
    })
}

pub fn run_whisper(samples: &[f32], model_path: &PathBuf, language: &str) -> Result<String> {
    let ctx = load_whisper(model_path)?;
    let text = transcribe_with(&ctx, samples, language)?;
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
    /// How often to refresh the live transcript.
    preview_interval: Duration,
}

impl TranscriptionCoordinator {
    pub fn new(
        app: AppHandle,
        state: SharedAppState,
        model_path: PathBuf,
        language: String,
    ) -> Self {
        TranscriptionCoordinator {
            app,
            state,
            model_path,
            language,
            // Fast enough to feel live, slow enough that each pass can finish.
            preview_interval: Duration::from_millis(450),
        }
    }

    /// Multilingual Hinglish passes are slow. Live updates would keep the mic
    /// open and delay paste until a full re-decode finishes, so Hinglish
    /// transcribes once, when the key is released.
    fn live_preview(&self) -> bool {
        !matches!(self.language.as_str(), "hi" | "hinglish")
    }

    async fn load_model(&self) -> Result<whisper_rs::WhisperContext> {
        let model_path = self.model_path.clone();
        tauri::async_runtime::spawn_blocking(move || load_whisper(&model_path))
            .await
            .map_err(|e| anyhow::anyhow!("ASR_MODEL_MISSING: {e}"))?
    }

    /// Transcribe without reloading the model. Hands the context back so the
    /// next pass stays fast.
    async fn transcribe(
        &self,
        ctx: whisper_rs::WhisperContext,
        samples: Vec<f32>,
    ) -> (whisper_rs::WhisperContext, Result<String>) {
        if samples.len() < (TARGET_SAMPLE_RATE / 4) as usize {
            return (ctx, Ok(String::new()));
        }
        let language = self.language.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            let text = transcribe_with(&ctx, &samples, &language);
            (ctx, text)
        })
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                let ctx = match self.load_model().await {
                    Ok(ctx) => ctx,
                    Err(_) => match self.load_model().await {
                        Ok(ctx) => ctx,
                        Err(err) => {
                            error!("Whisper context lost and reload failed: {err}");
                            // Blocking reload so the session can still finish.
                            match load_whisper(&self.model_path) {
                                Ok(ctx) => ctx,
                                Err(err2) => {
                                    error!("Whisper reload failed: {err2}");
                                    return (load_whisper(&self.model_path).unwrap_or_else(|_| {
                                        panic!("whisper model unusable: {e}");
                                    }), Err(anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}")));
                                }
                            }
                        }
                    },
                };
                (ctx, Err(anyhow::anyhow!("ASR_INFERENCE_FAILED: {e}")))
            }
        }
    }

    /// Push a live hypothesis into the focused field and the overlay.
    async fn emit_preview(
        &self,
        ctx: whisper_rs::WhisperContext,
        session_id: &str,
        samples: Vec<f32>,
        draft: &mut LiveDraft,
    ) -> whisper_rs::WhisperContext {
        let (ctx, text) = self.transcribe(ctx, samples).await;
        let text = match text {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => return ctx,
            Err(e) => {
                warn!("Preview inference error: {e}");
                return ctx;
            }
        };
        if !self.state.read().is_listening() {
            return ctx;
        }
        debug!("Live: {text}");
        self.state.write().update_preview(session_id, text.clone());
        let _ = self.app.emit(
            EVENT_PREVIEW,
            PreviewEvent {
                session_id: session_id.to_string(),
                text: text.clone(),
                is_provisional: true,
            },
        );
        if !draft.off() {
            if let Err(e) = draft.sync(&text).await {
                warn!("Live insert stopped, will paste on release: {e}");
            }
        }
        ctx
    }

    /// Run the final full-utterance pass and emit the final event.
    async fn emit_final(
        &self,
        ctx: whisper_rs::WhisperContext,
        session_id: &str,
        samples: Vec<f32>,
    ) -> (whisper_rs::WhisperContext, Result<String>) {
        info!(
            "Final pass: {} samples ({:.1} s)",
            samples.len(),
            samples.len() as f32 / TARGET_SAMPLE_RATE as f32
        );

        let (ctx, text) = self.transcribe(ctx, samples).await;
        let text = match text {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => {
                return (
                    ctx,
                    Err(anyhow::anyhow!("ASR_INFERENCE_FAILED: no speech detected")),
                );
            }
            Err(e) => return (ctx, Err(e)),
        };

        info!("Transcript: {text:?}");
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

        (ctx, Ok(text))
    }

    /// Returns true when the focused field already contains the final text,
    /// so the caller must not paste it a second time.
    pub async fn run(
        self,
        rx: AudioReceiver,
        session_id: String,
        mut stop_rx: oneshot::Receiver<bool>, // true = cancel
    ) -> bool {
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

        let mut ticker = interval(if self.live_preview() {
            self.preview_interval
        } else {
            Duration::from_secs(60)
        });
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut draft = LiveDraft::new();

        // Load while audio is already arriving. If the user lets go first,
        // fall through to a single final pass.
        let loaded = select! {
            biased;
            result = &mut stop_rx => {
                return self.finish(result, &session_id, &pcm, None, &mut draft).await;
            }
            loaded = self.load_model() => loaded,
        };
        let mut ctx = match loaded {
            Ok(ctx) => ctx,
            Err(e) => {
                error!("Model load failed: {e}");
                self.state.write().set_error(&session_id, e.to_string());
                let _ = self.app.emit(
                    EVENT_ERROR,
                    ErrorEvent {
                        session_id,
                        code: "ASR_MODEL_MISSING".into(),
                        message: e.to_string(),
                    },
                );
                return false;
            }
        };

        loop {
            select! {
                biased;
                result = &mut stop_rx => {
                    return self.finish(result, &session_id, &pcm, Some(ctx), &mut draft).await;
                }
                _ = ticker.tick(), if self.live_preview() => {
                    if !self.state.read().is_listening() {
                        continue;
                    }
                    let samples = pcm.lock().clone();
                    if samples.len() < TARGET_SAMPLE_RATE as usize / 3 {
                        continue;
                    }
                    ctx = self.emit_preview(ctx, &session_id, samples, &mut draft).await;
                }
            }
        }
    }

    /// `None` for the model means it is still loading (user released immediately).
    /// Returns true when the focused field already holds the final transcript.
    async fn finish(
        &self,
        result: std::result::Result<bool, tokio::sync::oneshot::error::RecvError>,
        session_id: &str,
        pcm: &Arc<parking_lot::Mutex<Vec<f32>>>,
        ctx: Option<whisper_rs::WhisperContext>,
        draft: &mut LiveDraft,
    ) -> bool {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let cancelled = result.unwrap_or(true);

        if cancelled {
            info!("Session {session_id} cancelled");
            if let Err(e) = draft.clear().await {
                warn!("Could not remove live draft: {e}");
            }
            self.state.write().cancel();
            let _ = self.app.emit(
                EVENT_STATE,
                StateEvent {
                    session_id: session_id.to_string(),
                    state: "IDLE".into(),
                },
            );
            return false;
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

        let ctx = match ctx {
            Some(ctx) => ctx,
            None => match self.load_model().await {
                Ok(ctx) => ctx,
                Err(e) => {
                    error!("Model load failed: {e}");
                    self.state.write().set_error(session_id, e.to_string());
                    return false;
                }
            },
        };

        let samples = pcm.lock().clone();
        let (_ctx, text) = self.emit_final(ctx, session_id, samples).await;
        match text {
            Ok(text) => match draft.sync(&text).await {
                Ok(()) => true,
                Err(e) => {
                    warn!("Could not update the live draft, will paste instead: {e}");
                    let _ = draft.clear().await;
                    false
                }
            },
            Err(e) => {
                error!("Final inference failed: {e}");
                // Keep whatever already landed in the field.
                if draft.has_text() {
                    true
                } else {
                    self.state.write().set_error(session_id, e.to_string());
                    let _ = self.app.emit(
                        EVENT_ERROR,
                        ErrorEvent {
                            session_id: session_id.to_string(),
                            code: "ASR_INFERENCE_FAILED".into(),
                            message: e.to_string(),
                        },
                    );
                    false
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

#[cfg(test)]
mod hinglish_tests {
    use super::romanize_hindi;

    #[test]
    fn keeps_english_and_romanizes_hindi() {
        assert_eq!(
            romanize_hindi("hello भाई, क्या कर रहे हो तुम?"),
            "hello bhai, kya kar rahe ho tum?"
        );
    }
}
