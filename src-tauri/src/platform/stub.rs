// VoiceKey – platform/stub.rs
// Compile-time stub for unsupported platforms.
// Linux is outside the MVP (TRD §18).

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
    compile_error!("VoiceKey only supports macOS and Windows");
}

pub async fn open_microphone_settings() {}
pub async fn open_accessibility_settings() {}

pub async fn insert_text_direct(_text: &str) -> Result<()> {
    Err(anyhow!("INSERTION_UNSUPPORTED: platform not supported"))
}

pub async fn insert_text_via_clipboard(_text: &str) -> Result<()> {
    Err(anyhow!("INSERTION_UNSUPPORTED: platform not supported"))
}

#[tauri::command]
pub async fn get_permission_status() -> PermissionStatus {
    check_permissions().await
}

#[tauri::command]
pub async fn request_microphone_permission() {}

#[tauri::command]
pub async fn request_accessibility_permission() {}
