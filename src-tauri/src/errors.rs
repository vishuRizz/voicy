// VoiceKey – errors.rs
// Structured error taxonomy (TRD §22).

use serde::Serialize;
use thiserror::Error;

/// All error codes surfaced to the UI.  Each variant maps to a user-facing
/// error code string and a localizable message.
#[derive(Error, Debug, Serialize, Clone)]
pub enum VoiceKeyError {
    #[error("Microphone permission denied. Please allow microphone access in System Settings → Privacy.")]
    MicPermissionDenied,

    #[error("No microphone device found. Please connect a microphone and try again.")]
    MicDeviceUnavailable,

    #[error("The global hotkey could not be registered: {0}")]
    HotkeyRegistrationFailed(String),

    #[error("The selected model is missing or incompatible: {0}")]
    ModelMissing(String),

    #[error("Transcription failed: {0}")]
    InferenceFailed(String),

    #[error("Accessibility permission missing. Please grant Accessibility access in System Settings → Privacy.")]
    InsertionPermissionMissing,

    #[error("Text insertion is not supported for this application. Clipboard fallback was used.")]
    InsertionUnsupported,

    #[error("Text insertion failed: {0}")]
    InsertionFailed(String),

    #[error("Session cancelled by user.")]
    SessionCancelled,
}

impl VoiceKeyError {
    /// Machine-readable error code emitted via the `session://error` event.
    pub fn code(&self) -> &'static str {
        match self {
            VoiceKeyError::MicPermissionDenied => "MIC_PERMISSION_DENIED",
            VoiceKeyError::MicDeviceUnavailable => "MIC_DEVICE_UNAVAILABLE",
            VoiceKeyError::HotkeyRegistrationFailed(_) => "HOTKEY_REGISTRATION_FAILED",
            VoiceKeyError::ModelMissing(_) => "ASR_MODEL_MISSING",
            VoiceKeyError::InferenceFailed(_) => "ASR_INFERENCE_FAILED",
            VoiceKeyError::InsertionPermissionMissing => "INSERTION_PERMISSION_MISSING",
            VoiceKeyError::InsertionUnsupported => "INSERTION_UNSUPPORTED",
            VoiceKeyError::InsertionFailed(_) => "INSERTION_FAILED",
            VoiceKeyError::SessionCancelled => "SESSION_CANCELLED",
        }
    }
}
