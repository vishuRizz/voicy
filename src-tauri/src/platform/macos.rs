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

/// Write text to clipboard via pbcopy, then send Cmd+V via CoreGraphics.
pub async fn insert_text_via_clipboard(text: &str) -> Result<()> {
    let previous = read_clipboard();
    write_clipboard(text)?;
    info!("Clipboard fallback: {} chars → Cmd+V", text.len());
    post_cmd_v();
    if let Some(prev) = previous {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        let _ = write_clipboard(&prev);
    }
    Ok(())
}

/// Post a Cmd+V key event pair to the HID event stream.
fn post_cmd_v() {
    unsafe {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventCreateKeyboardEvent(
                source: *const std::ffi::c_void,
                keycode: u16,
                key_down: bool,
            ) -> *mut std::ffi::c_void;
            fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
            fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
            fn CFRelease(cf: *mut std::ffi::c_void);
        }
        const K_CMD: u64 = 0x100000;
        const V_KEY: u16 = 9;
        const HID_TAP: u32 = 0;

        let down = CGEventCreateKeyboardEvent(std::ptr::null(), V_KEY, true);
        CGEventSetFlags(down, K_CMD);
        CGEventPost(HID_TAP, down);
        CFRelease(down);

        let up = CGEventCreateKeyboardEvent(std::ptr::null(), V_KEY, false);
        CGEventSetFlags(up, K_CMD);
        CGEventPost(HID_TAP, up);
        CFRelease(up);
    }
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
