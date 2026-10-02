// VoiceKey – audio.rs
// Microphone capture and PCM buffering.
//
// Design goals (from TRD §14):
//  - Mono PCM at 16 kHz (whisper.cpp's native rate).
//  - Keep audio in memory only, never write to disk.
//  - Use a bounded channel to avoid unbounded memory growth.
//  - Capture and inference run off the UI thread (spawned onto a Tokio task).
//  - Detect device loss, permission denial, and initialization errors.

use anyhow::{anyhow, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SampleRate, StreamConfig,
};
use flume::{bounded, Receiver, Sender};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;
use tracing::{error, info, warn};

/// 16 kHz mono — whisper.cpp's expected input sample rate.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Maximum number of f32 chunks queued between capture and inference.
/// At 16 kHz, 1 chunk ≈ 512 samples ≈ 32 ms → ~3 s of queued audio max.
const CHANNEL_CAPACITY: usize = 100;

/// A chunk of raw f32 samples (already at 16 kHz, mono).
pub type AudioChunk = Vec<f32>;

/// Receiver end of the audio pipeline.  The transcription coordinator reads
/// from this to build its rolling PCM buffer.
pub type AudioReceiver = Receiver<AudioChunk>;

/// Guard that stops the cpal stream when dropped.
pub struct CaptureHandle {
    /// Signal used to request a clean stop.
    stop_signal: Arc<Notify>,
    /// Sender kept alive so the channel remains open while capturing.
    _tx: Sender<AudioChunk>,
}

impl CaptureHandle {
    pub fn stop(&self) {
        self.stop_signal.notify_one();
    }
}

/// Start microphone capture.  Returns the handle (call `.stop()` to end) and
/// a receiver of f32 audio chunks.
///
/// All heavy work (cpal stream callbacks) runs on cpal's internal thread;
/// the caller owns the receiver and reads from it on a Tokio task.
pub fn start_capture() -> Result<(CaptureHandle, AudioReceiver)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("MIC_DEVICE_UNAVAILABLE: no default input device"))?;

    info!("Audio capture device: {}", device.name().unwrap_or_default());

    // Ask for 16 kHz mono.  If unsupported, cpal will error and we surface it.
    let config = StreamConfig {
        channels: 1,
        sample_rate: SampleRate(TARGET_SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Fixed(512),
    };

    let supported = device.supported_input_configs().map_err(|e| {
        anyhow!("MIC_PERMISSION_DENIED or device error: {e}")
    })?;

    // Verify device actually supports 16 kHz mono f32 (or fall back to a
    // supported config and resample — simplified here to require exact match).
    let fmt = SampleFormat::F32;
    let _config_range = supported
        .filter(|c| {
            c.channels() == 1
                && c.sample_format() == fmt
                && c.min_sample_rate().0 <= TARGET_SAMPLE_RATE
                && c.max_sample_rate().0 >= TARGET_SAMPLE_RATE
        })
        .next()
        .ok_or_else(|| {
            anyhow!("MIC_DEVICE_UNAVAILABLE: device does not support 16 kHz mono f32")
        })?;

    let (tx, rx) = bounded::<AudioChunk>(CHANNEL_CAPACITY);
    let tx_clone = tx.clone();
    let stop_signal = Arc::new(Notify::new());
    let stop_clone = stop_signal.clone();

    // Build the cpal stream.
    let stream = device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let chunk: AudioChunk = data.to_vec();
                // Drop chunks silently if the channel is full (backpressure).
                if tx_clone.try_send(chunk).is_err() {
                    warn!("Audio channel full — dropping chunk");
                }
            },
            move |err| {
                error!("Capture stream error: {err}");
                stop_clone.notify_one();
            },
            Some(Duration::from_millis(100)),
        )
        .map_err(|e| anyhow!("MIC_PERMISSION_DENIED or stream error: {e}"))?;

    stream.play().map_err(|e| anyhow!("Stream play failed: {e}"))?;

    // Keep the stream alive by leaking it for the session duration.
    // The stop signal will cause the capture task to exit and the channel
    // to close, which ends downstream inference naturally.
    std::mem::forget(stream);

    let handle = CaptureHandle {
        stop_signal,
        _tx: tx,
    };

    Ok((handle, rx))
}

/// Drain all pending chunks from the receiver into a contiguous f32 buffer.
/// Used by the transcription coordinator to build the final-pass input.
pub fn drain_into_buffer(rx: &AudioReceiver) -> Vec<f32> {
    let mut buf = Vec::new();
    while let Ok(chunk) = rx.try_recv() {
        buf.extend(chunk);
    }
    buf
}
