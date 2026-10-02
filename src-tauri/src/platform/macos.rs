// VoiceKey – platform/macos.rs
// macOS-specific implementations:
//  - Accessibility / text insertion via CGEvent synthetic key
//  - Clipboard-paste fallback via NSPasteboard
//  - Microphone & accessibility permission checks

use anyhow::{anyhow, Result};
use serde::Serialize;
use tracing::{info, warn};

// ── Permissions ──────────────────────────────────────────────────────────────

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

/// Check current microphone and accessibility permissions.
///
/// In production: use AVCaptureDevice.authorizationStatus for microphone and
/// AXIsProcessTrustedWithOptions for accessibility.  The stub below always
/// returns `NotDetermined` so the onboarding UI can guide the user.
pub async fn check_permissions() -> PermissionStatus {
    // TODO: replace stubs with real macOS API calls via objc crate.
    PermissionStatus {
        microphone: PermissionState::NotDetermined,
        accessibility: PermissionState::NotDetermined,
    }
}

/// Open System Settings → Privacy → Microphone (macOS 13+).
pub async fn open_microphone_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();
}

/// Open System Settings → Privacy → Accessibility.
pub async fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

// ── Text insertion ───────────────────────────────────────────────────────────

/// Attempt direct text insertion using macOS Accessibility APIs (CGEvent
/// synthetic key + AXUIElement setValue).
///
/// In production: use the `accessibility` or `core-graphics` crate to post
/// key events or set the focused element's AXValue directly.
pub async fn insert_text_direct(text: &str) -> Result<()> {
    // TODO: replace with real AX/CGEvent insertion.
    // Example approach:
    //   1. kAXFocusedUIElementAttribute → focused element
    //   2. AXUIElementSetAttributeValue(el, kAXValueAttribute, text)
    //   OR
    //   1. CGEventCreateKeyboardEvent per Unicode scalar
    //   2. CGEventPost(kCGHIDEventTap, event)
    warn!("insert_text_direct: stub — using clipboard fallback");
    Err(anyhow!("INSERTION_UNSUPPORTED: direct insertion not yet implemented"))
}

/// Clipboard-paste fallback: place text on NSPasteboard and send Cmd+V.
///
/// Saves and restores the previous clipboard contents where possible.
pub async fn insert_text_via_clipboard(text: &str) -> Result<()> {
    // Step 1: Save current clipboard (best-effort; may fail for complex types).
    let previous = read_clipboard();

    // Step 2: Write text to clipboard.
    write_clipboard(text)?;

    // Step 3: Simulate Cmd+V to the previously focused app.
    // In production: use CGEventCreateKeyboardEvent(Cmd+V) posted to kCGSessionEventTap.
    // Simplified here — in tests run `pbpaste` to verify.
    info!("Clipboard fallback: wrote {} chars, sending Cmd+V stub", text.len());

    // Step 4: Restore clipboard (best-effort).
    if let Some(prev) = previous {
        // Small delay to allow the paste to complete before restoring.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let _ = write_clipboard(&prev);
    }

    Ok(())
}

// ── Clipboard helpers (pbcopy / pbpaste for now) ─────────────────────────────

fn read_clipboard() -> Option<String> {
    std::process::Command::new("pbpaste")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .filter(|s| !s.is_empty())
}

fn write_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    let mut child = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

// ── Permission Tauri command ─────────────────────────────────────────────────

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
