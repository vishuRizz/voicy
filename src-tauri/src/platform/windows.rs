// VoiceKey – platform/windows.rs
// Windows-specific implementations (Phase 4).

use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PermissionStatus {
    pub microphone: PermissionState,
    pub accessibility: PermissionState,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
}

pub async fn check_permissions() -> PermissionStatus {
    // TODO: Windows.Media.Capture.MediaCapture for microphone.
    PermissionStatus {
        microphone: PermissionState::NotDetermined,
        accessibility: PermissionState::Granted, // Windows does not require explicit AX permission
    }
}

pub async fn open_microphone_settings() {
    let _ = std::process::Command::new("ms-settings:privacy-microphone").spawn();
}

pub async fn open_accessibility_settings() {
    let _ = std::process::Command::new("ms-settings:easeofaccess").spawn();
}

pub async fn insert_text_direct(text: &str) -> Result<()> {
    // TODO: Use SendInput() or IUIAutomationTextPattern.
    Err(anyhow!("INSERTION_UNSUPPORTED: Windows direct insertion not yet implemented"))
}

pub async fn insert_text_via_clipboard(text: &str) -> Result<()> {
    // TODO: Use SetClipboardData + SendInput(Ctrl+V).
    Err(anyhow!("INSERTION_UNSUPPORTED: Windows clipboard insertion not yet implemented"))
}

#[tauri::command]
pub async fn get_permission_status() -> PermissionStatus {
    check_permissions().await
}

#[tauri::command]
pub async fn request_microphone_permission() {
    open_microphone_settings().await;
}

#[tauri::command]
pub async fn request_accessibility_permission() {
    open_accessibility_settings().await;
}
