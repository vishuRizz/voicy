// VoiceKey – settings.rs
// Persists user preferences via the Tauri store plugin.

use serde::{Deserialize, Serialize};

/// Default global hold-to-talk shortcut.
pub const DEFAULT_SHORTCUT: &str = "Option+Space";

/// Available Whisper model sizes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WhisperModel {
    Tiny,
    Base,
    Small,
    Medium,
}

impl Default for WhisperModel {
    fn default() -> Self {
        WhisperModel::Base
    }
}

impl WhisperModel {
    pub fn filename(&self) -> &'static str {
        match self {
            WhisperModel::Tiny => "ggml-tiny.en.bin",
            WhisperModel::Base => "ggml-base.en.bin",
            WhisperModel::Small => "ggml-small.en.bin",
            WhisperModel::Medium => "ggml-medium.en.bin",
        }
    }

    pub fn size_mb(&self) -> u32 {
        match self {
            WhisperModel::Tiny => 75,
            WhisperModel::Base => 142,
            WhisperModel::Small => 466,
            WhisperModel::Medium => 1457,
        }
    }
}

/// All user-configurable settings persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Global hold-to-talk shortcut (e.g. "Option+Space").
    pub shortcut: String,
    /// Selected Whisper model variant.
    pub model: WhisperModel,
    /// BCP-47 language code (e.g. "en", "fr", "auto").
    pub language: String,
    /// Use clipboard-paste fallback when direct insertion is unavailable.
    pub clipboard_fallback: bool,
    /// Show the overlay window during recording.
    pub show_overlay: bool,
    /// Maximum recording duration in seconds (0 = unlimited).
    pub max_recording_secs: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shortcut: DEFAULT_SHORTCUT.to_string(),
            model: WhisperModel::default(),
            language: "en".to_string(),
            clipboard_fallback: true,
            show_overlay: true,
            max_recording_secs: 0,
        }
    }
}

/// Tauri commands exposed to the UI.
pub mod commands {
    use super::Settings;
    use tauri::AppHandle;
    use tauri_plugin_store::StoreExt;

    const STORE_PATH: &str = "settings.json";
    const SETTINGS_KEY: &str = "settings";

    /// Load settings from the persistent store (or defaults).
    #[tauri::command]
    pub async fn get_settings(app: AppHandle) -> Result<Settings, String> {
        let store = app
            .store(STORE_PATH)
            .map_err(|e| format!("store open failed: {e}"))?;

        let settings: Settings = store
            .get(SETTINGS_KEY)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        Ok(settings)
    }

    /// Persist updated settings.
    #[tauri::command]
    pub async fn update_settings(
        app: AppHandle,
        settings: Settings,
    ) -> Result<(), String> {
        let store = app
            .store(STORE_PATH)
            .map_err(|e| format!("store open failed: {e}"))?;

        let value = serde_json::to_value(&settings)
            .map_err(|e| format!("serialize failed: {e}"))?;

        store
            .set(SETTINGS_KEY, value);
        store
            .save()
            .map_err(|e| format!("store save failed: {e}"))?;

        Ok(())
    }
}
