// VoiceKey – platform/macos.rs
// macOS-specific implementations.

use anyhow::{anyhow, Result};
use block::ConcreteBlock;
use objc::runtime::Class;
use serde::Serialize;
use tracing::{info, warn};

// ── Process name (dev-mode fix) ───────────────────────────────────────────────

/// Set the process display name to "VoiceKey" so macOS shows it correctly
/// in System Settings → Privacy lists, even in dev mode without a .app bundle.
pub fn set_process_name() {
    unsafe {
        let process_info: *mut objc::runtime::Object =
            msg_send![Class::get("NSProcessInfo").unwrap(), processInfo];
        let name = "VoiceKey\0";
        let ns_name: *mut objc::runtime::Object =
            msg_send![Class::get("NSString").unwrap(), stringWithUTF8String: name.as_ptr()];
        let _: () = msg_send![process_info, setProcessName: ns_name];
    }
    info!("Process name set to VoiceKey");
}

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

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    // Pass NULL options for a silent check, or a dict with
    // kAXTrustedCheckOptionPrompt = true to show the system dialog.
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

fn ax_is_trusted() -> bool {
    unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) }
}

/// Trigger the real macOS accessibility prompt.
/// Passes kAXTrustedCheckOptionPrompt = kCFBooleanTrue so the system
/// dialog appears and adds VoiceKey to the Accessibility list by name.
fn ax_prompt_for_trust() {
    unsafe {
        // Build a CFDictionary: { kAXTrustedCheckOptionPrompt: kCFBooleanTrue }
        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            static kCFBooleanTrue: *const std::ffi::c_void;
            fn CFDictionaryCreate(
                allocator: *const std::ffi::c_void,
                keys: *const *const std::ffi::c_void,
                values: *const *const std::ffi::c_void,
                count: isize,
                key_callbacks: *const std::ffi::c_void,
                value_callbacks: *const std::ffi::c_void,
            ) -> *const std::ffi::c_void;
            fn CFRelease(cf: *const std::ffi::c_void);
            static kCFTypeDictionaryKeyCallBacks: std::ffi::c_void;
            static kCFTypeDictionaryValueCallBacks: std::ffi::c_void;
        }

        // kAXTrustedCheckOptionPrompt as a CFString
        let key_str = "AXTrustedCheckOptionPrompt\0";
        let nsstring_cls = Class::get("NSString").unwrap();
        let alloc: *mut objc::runtime::Object = msg_send![nsstring_cls, alloc];
        let key_cfstr: *const std::ffi::c_void = msg_send![alloc,
            initWithUTF8String: key_str.as_ptr()
        ];

        let keys = [key_cfstr];
        let values = [kCFBooleanTrue];

        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const _,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const _,
        );

        AXIsProcessTrustedWithOptions(dict);
        CFRelease(dict);
        CFRelease(key_cfstr);
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
    // First trigger the real system prompt (adds us by name to the list)
    ax_prompt_for_trust();
    // Then open Settings so user sees the toggle
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
pub async fn request_microphone_permission(app: tauri::AppHandle) {
    use tauri::Emitter;

    // Fire AVCaptureDevice.requestAccess(for: .audio).
    // In the completion block, emit the updated permission status so the UI
    // refreshes immediately when the user taps Allow — no manual restart needed.
    let app_clone = app.clone();
    std::thread::spawn(move || unsafe {
        let cls = match Class::get("AVCaptureDevice") {
            Some(c) => c,
            None => return,
        };
        let nsstring_cls = match Class::get("NSString") {
            Some(c) => c,
            None => return,
        };
        let av_audio = "soun"; // AVMediaTypeAudio
        let alloc: *mut objc::runtime::Object = msg_send![nsstring_cls, alloc];
        let ns_media_type: *mut objc::runtime::Object = msg_send![alloc,
            initWithBytes: av_audio.as_ptr()
            length: av_audio.len()
            encoding: 4u64
        ];

        let app_for_block = app_clone.clone();
        let block = ConcreteBlock::new(move |granted: bool| {
            info!("Microphone permission response: granted={granted}");
            // Re-read full status and push to the UI.
            let status = PermissionStatus {
                microphone: if granted {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                },
                accessibility: if ax_is_trusted() {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                },
            };
            let _ = Emitter::emit(&app_for_block, "permissions://status", &status);
        })
        .copy();

        let _: () = msg_send![cls,
            requestAccessForMediaType: ns_media_type
            completionHandler: &*block
        ];
        let _: () = msg_send![ns_media_type, release];
    });

    // Open System Settings after a short pause so user sees the toggle.
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    open_microphone_settings().await;
}

#[tauri::command]
pub async fn request_accessibility_permission(app: tauri::AppHandle) {
    use tauri::Emitter;
    open_accessibility_settings().await;
    // Re-emit status after opening settings (user may have already granted).
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let status = check_permissions().await;
    let _ = Emitter::emit(&app, "permissions://status", &status);
}
