// VoiceKey – platform/macos.rs
// macOS-specific implementations.

use anyhow::{anyhow, Result};
use block::ConcreteBlock;
use objc::runtime::{Class, Object};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::OnceLock;
use tracing::{info, warn};

// ── Process name (dev-mode fix) ───────────────────────────────────────────────

/// Set the process display name to "Voicy" so macOS shows it correctly
/// in System Settings → Privacy lists, even in dev mode without a .app bundle.
pub fn set_process_name() {
    unsafe {
        let process_info: *mut objc::runtime::Object =
            msg_send![Class::get("NSProcessInfo").unwrap(), processInfo];
        let name = "Voicy\0";
        let ns_name: *mut objc::runtime::Object =
            msg_send![Class::get("NSString").unwrap(), stringWithUTF8String: name.as_ptr()];
        let _: () = msg_send![process_info, setProcessName: ns_name];
    }
    info!("Process name set to Voicy");
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
    // A deleted or rebuilt Voicy can stay listed as On. That grant does not
    // apply to this copy, so reset it before asking again.
    for id in ["com.voicekey.app", "Voicy", "VoiceKey"] {
        let result = std::process::Command::new("tccutil")
            .args(["reset", "Accessibility", id])
            .output();
        match result {
            Ok(o) if o.status.success() => {
                info!("Cleared stale Accessibility grant for {id}");
            }
            Ok(o) => {
                let msg = String::from_utf8_lossy(&o.stderr);
                tracing::debug!("tccutil reset {id}: {msg}");
            }
            Err(e) => tracing::debug!("tccutil unavailable: {e}"),
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = exe.to_str() {
            let _ = std::process::Command::new("tccutil")
                .args(["reset", "Accessibility", path])
                .output();
        }
    }

    ax_prompt_for_trust();
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

    // If VoiceKey itself is the front app, Cmd+V would land in this window.
    // Hide it so the app the user was typing in becomes frontmost.
    let stepped_aside = step_aside_if_frontmost();

    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    info!("Clipboard paste: {} chars → Cmd+V via osascript", text.len());

    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .output()
        .map_err(|e| anyhow!("osascript not available: {e}"))?;

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stderr);
        if stepped_aside {
            unhide_without_activating();
        }
        return Err(anyhow!(
            "Paste failed ({msg}). Grant Accessibility to Voicy in System Settings → Privacy & Security → Accessibility."
        ));
    }

    // Restore previous clipboard contents after paste has had time to land.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if let Some(prev) = previous {
        let _ = write_clipboard(&prev);
    }
    if stepped_aside {
        unhide_without_activating();
    }

    Ok(())
}

fn step_aside_if_frontmost() -> bool {
    unsafe {
        let Some(cls) = Class::get("NSApplication") else { return false };
        let app: *mut Object = msg_send![cls, sharedApplication];
        if app.is_null() {
            return false;
        }
        let active: bool = msg_send![app, isActive];
        if !active {
            return false;
        }
        info!("Voicy is frontmost — hiding so paste lands in the previous app");
        let _: () = msg_send![app, hide: std::ptr::null::<*mut Object>()];
        true
    }
}

/// Replace the tail of the live transcript in the focused field.
///
/// Backspaces and Cmd+V are posted with an explicit modifier mask, so a
/// shortcut the user is still holding (Ctrl, Option, …) is not mixed in.
pub fn apply_text_edit(delete_chars: usize, insert: &str) -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    if delete_chars == 0 && insert.is_empty() {
        return Ok(());
    }
    if !insert.is_empty() {
        write_clipboard(insert)?;
    }

    fn post_key(code: u16, flags: CGEventFlags) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("could not create keyboard event source"))?;
        let event = CGEvent::new_keyboard_event(source, code, true)
            .map_err(|_| anyhow!("could not create keyboard event"))?;
        event.set_flags(flags);
        event.post(CGEventTapLocation::HID);

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("could not create keyboard event source"))?;
        let event = CGEvent::new_keyboard_event(source, code, false)
            .map_err(|_| anyhow!("could not create keyboard event"))?;
        event.set_flags(flags);
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    for _ in 0..delete_chars {
        // Delete, with no modifiers, so we only remove our own draft.
        post_key(51, CGEventFlags::CGEventFlagNull)?;
    }
    if !insert.is_empty() {
        // Let the clipboard and the backspaces land before pasting.
        std::thread::sleep(std::time::Duration::from_millis(if delete_chars == 0 { 20 } else { 30 }));
        post_key(9, CGEventFlags::CGEventFlagCommand)?; // V
    }
    Ok(())
}

fn unhide_without_activating() {
    unsafe {
        let Some(cls) = Class::get("NSApplication") else { return };
        let app: *mut Object = msg_send![cls, sharedApplication];
        if app.is_null() {
            return;
        }
        let _: () = msg_send![app, unhideWithoutActivation];
    }
}

// ── Physical key-up (hold-to-talk release) ───────────────────────────────────
//
// Carbon's hotkey release is not delivered while another app is focused, so
// the session never left LISTENING and nothing was pasted.

const OPT: u64 = 1 << 19;
const CTRL: u64 = 1 << 18;
const SHIFT: u64 = 1 << 17;
const CMD: u64 = 1 << 20;

static RELEASE_CB: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static RELEASE_ARMED: AtomicBool = AtomicBool::new(false);
static RELEASE_KEY: AtomicU16 = AtomicU16::new(u16::MAX);
static RELEASE_MODS: AtomicU64 = AtomicU64::new(0);

/// Install a process-lifetime key-up monitor. The callback fires once per
/// release while a watch is armed. Must be called on the main thread.
pub fn install_release_watch(on_release: impl Fn() + Send + Sync + 'static) {
    let _ = RELEASE_CB.set(Box::new(on_release));
    install_monitors();
}

/// Start treating a physical release of `shortcut` (e.g. "Alt+Space") as
/// the end of hold-to-talk.
pub fn arm_release_watch(shortcut: &str) {
    let (key, mods) = shortcut_watch(shortcut);
    RELEASE_KEY.store(key, Ordering::Relaxed);
    RELEASE_MODS.store(mods, Ordering::Relaxed);
    RELEASE_ARMED.store(true, Ordering::SeqCst);
    info!("Watching physical release of {shortcut} (key={key}, mods={mods:#x})");
}

pub fn clear_release_watch() {
    RELEASE_ARMED.store(false, Ordering::SeqCst);
}

fn install_monitors() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| unsafe {
        // NSEventTypeKeyUp = 11, NSEventTypeFlagsChanged = 12.
        let mask: usize = (1 << 11) | (1 << 12);
        let cls = match Class::get("NSEvent") {
            Some(c) => c,
            None => return,
        };

        let global_block = ConcreteBlock::new(move |event: *mut Object| {
            note_release_event(event);
        })
        .copy();
        let local_block = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
            note_release_event(event);
            event
        })
        .copy();

        let global: *mut Object = msg_send![cls,
            addGlobalMonitorForEventsMatchingMask: mask
            handler: &*global_block
        ];
        let _local: *mut Object = msg_send![cls,
            addLocalMonitorForEventsMatchingMask: mask
            handler: &*local_block
        ];
        if global.is_null() {
            warn!(
                "Global key-up monitor was not installed. Grant Accessibility \
                 so releasing the shortcut works while another app is focused."
            );
        }
        // AppKit keeps these for the life of the process.
        std::mem::forget(global_block);
        std::mem::forget(local_block);
    });
}

fn note_release_event(event: *mut Object) {
    if event.is_null() || !release_event_matches(event) {
        return;
    }
    // Only the first matching event for this hold ends the session.
    if !RELEASE_ARMED.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(cb) = RELEASE_CB.get() {
        cb();
    }
}

fn release_event_matches(event: *mut Object) -> bool {
    if !RELEASE_ARMED.load(Ordering::SeqCst) {
        return false;
    }
    unsafe {
        let code: u16 = msg_send![event, keyCode];
        let flags: u64 = msg_send![event, modifierFlags];
        let expected = RELEASE_KEY.load(Ordering::Relaxed);
        let mods = RELEASE_MODS.load(Ordering::Relaxed);

        // The shortcut's own key went up (Space, J, …).
        if code == expected {
            return true;
        }

        // A modifier key went up and the required modifiers are no longer held.
        // Do not treat other key-ups this way. Live transcription posts Cmd+V,
        // and those key-ups do not include Option/Ctrl. Counting them as a
        // release stopped the recording after the first phrase.
        is_modifier_key(code) && mods != 0 && (flags & mods) != mods
    }
}

fn is_modifier_key(code: u16) -> bool {
    // Left/right command, shift, option, control, caps lock, fn.
    matches!(code, 54 | 55 | 56 | 57 | 58 | 59 | 60 | 61 | 62 | 63)
}

fn shortcut_watch(shortcut: &str) -> (u16, u64) {
    let mut mods = 0u64;
    let mut key = u16::MAX;
    for part in shortcut.split('+') {
        match part {
            "Alt" | "Option" => mods |= OPT,
            "Ctrl" | "Control" => mods |= CTRL,
            "Shift" => mods |= SHIFT,
            "Super" | "Command" | "Cmd" | "Meta" => mods |= CMD,
            other => {
                if let Some(code) = mac_key_code(other) {
                    key = code;
                }
            }
        }
    }
    (key, mods)
}

fn mac_key_code(name: &str) -> Option<u16> {
    Some(match name {
        "A" => 0, "S" => 1, "D" => 2, "F" => 3, "H" => 4, "G" => 5,
        "Z" => 6, "X" => 7, "C" => 8, "V" => 9, "B" => 11, "Q" => 12,
        "W" => 13, "E" => 14, "R" => 15, "Y" => 16, "T" => 17,
        "1" => 18, "2" => 19, "3" => 20, "4" => 21, "6" => 22, "5" => 23,
        "Equal" => 24, "9" => 25, "7" => 26, "Minus" => 27, "8" => 28, "0" => 29,
        "O" => 31, "U" => 32, "I" => 34, "P" => 35, "Return" | "Enter" => 36,
        "L" => 37, "J" => 38, "K" => 40, "N" => 45, "M" => 46,
        "Tab" => 48, "Space" => 49, "Escape" => 53,
        "F1" => 122, "F2" => 120, "F3" => 99, "F4" => 118, "F5" => 96,
        "F6" => 97, "F7" => 98, "F8" => 100, "F9" => 101, "F10" => 109,
        "F11" => 103, "F12" => 111,
        _ => return None,
    })
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
