// VoiceKey – audio.rs
// Microphone capture and PCM buffering.
//
// Design goals (from TRD §14):
//  - Mono PCM at 16 kHz (whisper.cpp's native rate).
//  - Keep audio in memory only, never write to disk.
//  - Use a bounded channel to avoid unbounded memory growth.
//  - Capture and inference run off the UI thread (spawned onto a Tokio task).
//  - Detect device loss, permission denial, and initialization errors.
//  - Flexible config: accept any sample rate / channel count and resample down.

use anyhow::{anyhow, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SampleRate, StreamConfig,
};
use dasp_sample::Sample;
use flume::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use tracing::{error, info, warn};

/// 16 kHz mono — whisper.cpp's expected input sample rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Maximum number of f32 chunks queued between capture and inference.
/// Long enough that a slow transcription pass cannot drop the rest of a sentence.
const CHANNEL_CAPACITY: usize = 4_000;

/// A chunk of raw f32 samples (already at 16 kHz, mono).
pub type AudioChunk = Vec<f32>;

/// Receiver end of the audio pipeline.
pub type AudioReceiver = Receiver<AudioChunk>;

/// Guard that stops the cpal stream when dropped.
///
/// The stream itself lives on the thread that created it (`cpal::Stream` is
/// not `Send`). `stop` asks that thread to drop it.
pub struct CaptureHandle {
    stop_tx: mpsc::Sender<()>,
}

impl CaptureHandle {
    pub fn stop(&self) {
        let _ = self.stop_tx.send(());
    }

    /// Another copy of the stop signal, so key-up can halt the mic
    /// without waiting for a transcription pass to finish.
    pub fn stop_sender(&self) -> mpsc::Sender<()> {
        self.stop_tx.clone()
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
    }
}

/// Start microphone capture.
///
/// Accepts *any* supported config and downmixes/resamples to 16 kHz mono.
/// Returns (handle, receiver).
pub fn start_capture() -> Result<(CaptureHandle, AudioReceiver)> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel();
    let (tx, rx) = bounded::<AudioChunk>(CHANNEL_CAPACITY);

    std::thread::Builder::new()
        .name("voicekey-audio".into())
        .spawn(move || {
            let started = open_input_stream(tx);
            match started {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    // Block until the session asks us to stop, then drop the stream
                    // on this same thread.
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .context("failed to spawn audio thread")?;

    ready_rx
        .recv()
        .context("audio thread exited before the microphone opened")??;

    info!("Capture started ✓");
    Ok((CaptureHandle { stop_tx }, rx))
}

fn note_dropped_chunk() {
    static DROPS: AtomicU64 = AtomicU64::new(0);
    let n = DROPS.fetch_add(1, Ordering::Relaxed);
    if n % 50 == 0 {
        warn!("Audio channel full — dropping chunks ({n} so far)");
    }
}

/// Open the default input device and return a playing stream.
/// Must be dropped on the same thread that created it.
fn open_input_stream(tx: Sender<AudioChunk>) -> Result<cpal::Stream> {
    let host = cpal::default_host();

    let device = host
        .default_input_device()
        .ok_or_else(|| {
            #[cfg(target_os = "macos")]
            let hint = "On macOS, grant Microphone access in System Settings → Privacy → Microphone.";
            #[cfg(target_os = "windows")]
            let hint = "On Windows, grant Microphone access in Settings → Privacy → Microphone.";
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let hint = "Check your system's microphone privacy settings.";
            anyhow!("MIC_NOT_FOUND: No microphone detected. {hint}")
        })?;

    let dev_name = device.name().unwrap_or_else(|_| "unknown".into());
    info!("Audio device: {dev_name}");

    // ── Pick the best available supported config ──────────────────────────
    #[cfg(target_os = "macos")]
    let mic_enum_err = "MIC_PERMISSION_DENIED: Could not enumerate input configs. Grant Microphone access in System Settings → Privacy → Microphone.";
    #[cfg(target_os = "windows")]
    let mic_enum_err = "MIC_PERMISSION_DENIED: Could not enumerate input configs. Grant Microphone access in Settings → Privacy → Microphone.";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mic_enum_err = "MIC_PERMISSION_DENIED: Could not enumerate input configs.";

    let supported: Vec<_> = device
        .supported_input_configs()
        .context(mic_enum_err)?
        .collect();

    if supported.is_empty() {
        return Err(anyhow!(
            "MIC_NOT_FOUND: Microphone found but reports no supported configurations. \
             Try reconnecting the device."
        ));
    }

    // Prefer F32 at or near 16 kHz mono; fall back to any format.
    let best = supported
        .iter()
        .find(|c| c.sample_format() == SampleFormat::F32 && c.channels() == 1)
        .or_else(|| supported.iter().find(|c| c.sample_format() == SampleFormat::F32))
        .or_else(|| supported.first())
        .unwrap()
        .clone();

    // Clamp requested rate to device limits.
    let native_rate = TARGET_SAMPLE_RATE
        .max(best.min_sample_rate().0)
        .min(best.max_sample_rate().0);

    let channels = best.channels();
    let format  = best.sample_format();

    let config = StreamConfig {
        channels,
        sample_rate: SampleRate(native_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    info!("Capture config: {native_rate} Hz × {channels}ch, format={format:?}");

    let tx_clone = tx.clone();

    // ── Build stream — handle format + channel conversion inline ──────────
    let build_err = |e: cpal::BuildStreamError| {
        #[cfg(target_os = "macos")]
        let hint = "Grant Microphone access in System Settings → Privacy → Microphone.";
        #[cfg(target_os = "windows")]
        let hint = "Grant Microphone access in Settings → Privacy → Microphone.";
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let hint = "Check your system's microphone privacy settings.";
        anyhow!("MIC_PERMISSION_DENIED: Could not open microphone stream: {e}. {hint}")
    };

    macro_rules! build_stream {
        ($t:ty) => {{
            let tx_clone = tx_clone.clone();
            device.build_input_stream(
                &config,
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    // Downmix to mono + convert to f32
                    let mono: Vec<f32> = data
                        .chunks(channels as usize)
                        .map(|frame| {
                            let sum: f32 = frame
                                .iter()
                                .map(|s| (*s).to_sample::<f32>())
                                .sum::<f32>();
                            sum / channels as f32
                        })
                        .collect();

                    // Simple linear resample if needed
                    let resampled = if native_rate == TARGET_SAMPLE_RATE {
                        mono
                    } else {
                        resample(&mono, native_rate, TARGET_SAMPLE_RATE)
                    };

                    if tx_clone.try_send(resampled).is_err() {
                        note_dropped_chunk();
                    }
                },
                move |err| {
                    error!("Capture stream error: {err}");
                },
                Some(Duration::from_millis(100)),
            )
            .map_err(build_err)
        }};
    }

    let stream = match format {
        SampleFormat::F32 => build_stream!(f32)?,
        SampleFormat::I16 => build_stream!(i16)?,
        SampleFormat::U16 => build_stream!(u16)?,
        _ => {
            // Newer cpal versions expose more formats — fall back to F32
            warn!("Unexpected sample format {format:?}, trying F32");
            let f32_config = StreamConfig {
                channels,
                sample_rate: SampleRate(native_rate),
                buffer_size: cpal::BufferSize::Default,
            };
            device
                .build_input_stream(
                    &f32_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mono: Vec<f32> = data
                            .chunks(channels as usize)
                            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                            .collect();
                        let out = if native_rate == TARGET_SAMPLE_RATE { mono }
                                  else { resample(&mono, native_rate, TARGET_SAMPLE_RATE) };
                        if tx.try_send(out).is_err() {
                            note_dropped_chunk();
                        }
                    },
                    move |err| { error!("Stream error: {err}"); },
                    Some(Duration::from_millis(100)),
                )
                .map_err(build_err)?
        }
    };

    stream.play().map_err(|e| anyhow!("Stream play failed: {e}"))?;
    Ok(stream)
}

/// Naive linear interpolation resampler.
/// Good enough for downsampling from common rates (44100/48000 → 16000).
fn resample(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_len = (input.len() as f64 / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let lo  = src.floor() as usize;
        let hi  = (lo + 1).min(input.len() - 1);
        let frac = (src - lo as f64) as f32;
        out.push(input[lo] * (1.0 - frac) + input[hi] * frac);
    }
    out
}

/// Drain all pending chunks from the receiver into a contiguous f32 buffer.
pub fn drain_into_buffer(rx: &AudioReceiver) -> Vec<f32> {
    let mut buf = Vec::new();
    while let Ok(chunk) = rx.try_recv() {
        buf.extend(chunk);
    }
    buf
}
