// VoiceKey – platform/macos.rs
// macOS-specific implementations.

use anyhow::{anyhow, Result};
use objc::runtime::Class;
use serde::Serialize;
use tracing::{info, warn};

// ── Permission types ─────────────────────────────────────────────────────────

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

// ── AVFoundation microphone permission check ─────────────────────────────────

/// AVAuthorizationStatus: 0=NotDetermined, 1=Restricted, 2=Denied, 3=Authorized
fn avfoundation_mic_status() -> PermissionState {
    unsafe {
        let cls = match Class::get("AVCaptureDevice") {
            Some(c) => c,
            None => return PermissionState::NotDetermined,
        };
        let nsstring_cls = match Class::get("NSString") {
            Some(c) => c,
            None => return PermissionState::NotDetermined,
        };

        // Build NSString for "soun" (AVMediaTypeAudio)
        let av_audio = "soun";
        let alloc: *mut objc::runtime::Object = msg_send![nsstring_cls, alloc];
        let ns_media_type: *mut objc::runtime::Object = msg_send![alloc,
            initWithBytes: av_audio.as_ptr()
            length: av_audio.len()
            encoding: 4u64   // NSUTF8StringEncoding
        ];

        let status: i64 = msg_send![cls, authorizationStatusForMediaType: ns_media_type];
        let _: () = msg_send![ns_media_type, release];

        match status {
            3 => PermissionState::Granted,
            1 | 2 => PermissionState::Denied,
            _ => PermissionState::NotDetermined,
        }
    }
}

// ── Accessibility ────────────────────────────────────────────────────────────

fn ax_is_trusted() -> bool {
    unsafe {
        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        AXIsProcessTrusted()
    }
}

// ── Public API ───────────────────────────────────────────────────────────────

pub async fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        microphone: avfoundation_mic_status(),
        accessibility: if ax_is_trusted() {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        },
    }
}

pub async fn open_microphone_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();
}

pub async fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

// ── Text insertion ───────────────────────────────────────────────────────────

pub async fn insert_text_direct(_text: &str) -> Result<()> {
    warn!("insert_text_direct: not yet implemented — using clipboard fallback");
    Err(anyhow!("INSERTION_UNSUPPORTED: direct insertion not yet implemented"))
}

/// Write text to clipboard via pbcopy, then send Cmd+V via osascript.
///
/// osascript targets the *frontmost* application. Because the overlay window
/// has `acceptFirstMouse: false` and never takes focus, the app the user was
/// typing in remains frontmost — so the paste lands in the right place.
///
/// Requires: System Settings → Privacy → Accessibility → VoiceKey ✅
pub async fn insert_text_via_clipboard(text: &str) -> Result<()> {
    let previous = read_clipboard();

    write_clipboard(text)?;

    // Give the clipboard a moment to settle before pasting.
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    info!("Clipboard paste: {} chars → Cmd+V via osascript", text.len());

    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let msg = String::from_utf8_lossy(&o.stderr);
            warn!("osascript paste failed: {msg}");
        }
        Err(e) => {
            warn!("osascript not available: {e}");
        }
    }

    // Restore previous clipboard contents after paste has had time to land.
    if let Some(prev) = previous {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let _ = write_clipboard(&prev);
    }

    Ok(())
}

// ── Clipboard helpers ────────────────────────────────────────────────────────

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

// ── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_permission_status() -> PermissionStatus {
    check_permissions().await
}

#[tauri::command]
pub async fn request_microphone_permission() {
    // Opening System Settings triggers a permission check for the calling process.
    // The first time cpal actually opens the mic, macOS shows the system prompt.
    // We open the settings pane so users see the toggle the moment it appears.
    open_microphone_settings().await;
}

#[tauri::command]
pub async fn request_accessibility_permission() {
    open_accessibility_settings().await;
}
