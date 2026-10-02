// VoiceKey – platform/windows.rs
// Full Windows implementation:
//   • Clipboard write + Ctrl+V via SendInput
//   • Backspace × N + Ctrl+V for live edits (apply_text_edit)
//   • Low-level WH_KEYBOARD_LL hook for hold-to-talk key-up detection
//   • Microphone permission check via Windows registry

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use tracing::{info, warn};

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT,
    WH_KEYBOARD_LL, WM_KEYUP, WM_SYSKEYUP,
};

// ── Permission types ──────────────────────────────────────────────────────────

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

// ── Permission check ──────────────────────────────────────────────────────────

/// Check microphone permission via the Windows CapabilityAccessManager registry key.
fn win_microphone_status() -> PermissionState {
    use windows::Win32::System::Registry::{
        RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
    };
    use windows::core::PCWSTR;

    let path: Vec<u16> = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone\0"
        .encode_utf16()
        .collect();
    let value_name: Vec<u16> = "Value\0".encode_utf16().collect();

    unsafe {
        let mut hkey = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );
        if result.is_err() {
            return PermissionState::NotDetermined;
        }

        let mut data_type = 0u32;
        let mut data = 0u32;
        let mut data_size = std::mem::size_of::<u32>() as u32;

        let query = RegQueryValueExW(
            hkey,
            PCWSTR(value_name.as_ptr()),
            None,
            Some(&mut data_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_size),
        );

        if query.is_err() {
            return PermissionState::NotDetermined;
        }

        if data_type == REG_DWORD.0 {
            if data == 1 { PermissionState::Granted } else { PermissionState::Denied }
        } else {
            PermissionState::NotDetermined
        }
    }
}

pub async fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        microphone: win_microphone_status(),
        // Windows does not require explicit Accessibility permission for SendInput.
        accessibility: PermissionState::Granted,
    }
}

pub async fn open_microphone_settings() {
    let _ = std::process::Command::new("explorer")
        .arg("ms-settings:privacy-microphone")
        .spawn();
}

pub async fn open_accessibility_settings() {
    let _ = std::process::Command::new("explorer")
        .arg("ms-settings:easeofaccess-keyboard")
        .spawn();
}

// ── Clipboard helpers ─────────────────────────────────────────────────────────

/// Write UTF-16 text to the Windows clipboard (CF_UNICODETEXT = 13).
fn write_clipboard(text: &str) -> Result<()> {
    unsafe {
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        utf16.push(0); // null terminator
        let byte_len = utf16.len() * 2;

        let hmem = GlobalAlloc(GMEM_MOVEABLE, byte_len)
            .map_err(|e| anyhow!("GlobalAlloc failed: {e}"))?;

        {
            let ptr = GlobalLock(hmem) as *mut u16;
            if ptr.is_null() {
                return Err(anyhow!("GlobalLock failed"));
            }
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
            let _ = GlobalUnlock(hmem); // non-fatal at lock count 0
        }

        OpenClipboard(HWND(std::ptr::null_mut()))
            .map_err(|e| anyhow!("OpenClipboard failed: {e}"))?;
        EmptyClipboard().map_err(|e| {
            let _ = CloseClipboard();
            anyhow!("EmptyClipboard failed: {e}")
        })?;
        SetClipboardData(13, HANDLE(hmem.0 as *mut _))
            .map_err(|e| {
                let _ = CloseClipboard();
                anyhow!("SetClipboardData failed: {e}")
            })?;
        CloseClipboard().map_err(|e| anyhow!("CloseClipboard failed: {e}"))?;
    }
    Ok(())
}

/// Read current clipboard text (CF_UNICODETEXT = 13).
fn read_clipboard() -> Option<String> {
    unsafe {
        if OpenClipboard(HWND(std::ptr::null_mut())).is_err() {
            return None;
        }
        let hmem = GetClipboardData(13).ok()?;
        let ptr = GlobalLock(HANDLE(hmem.0)) as *const u16;
        let result = if ptr.is_null() {
            None
        } else {
            let mut len = 0usize;
            while *ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(ptr, len);
            String::from_utf16(slice).ok()
        };
        let _ = GlobalUnlock(HANDLE(hmem.0));
        let _ = CloseClipboard();
        result
    }
}

// ── SendInput helpers ─────────────────────────────────────────────────────────

fn make_key_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Press and release `vk`, optionally with Ctrl held down.
fn send_key(vk: VIRTUAL_KEY, with_ctrl: bool) -> Result<()> {
    let mut inputs: Vec<INPUT> = Vec::new();
    if with_ctrl {
        inputs.push(make_key_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)));
    }
    inputs.push(make_key_input(vk, KEYBD_EVENT_FLAGS(0)));
    inputs.push(make_key_input(vk, KEYEVENTF_KEYUP));
    if with_ctrl {
        inputs.push(make_key_input(VK_CONTROL, KEYEVENTF_KEYUP));
    }
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(anyhow!("SendInput: sent {sent}/{} events", inputs.len()));
    }
    Ok(())
}

// ── Text insertion ────────────────────────────────────────────────────────────

/// Clipboard + Ctrl+V paste (primary method on Windows).
///
/// Saves and restores the previous clipboard content so the user's copy
/// buffer is not permanently replaced.
pub async fn insert_text_via_clipboard(text: &str) -> Result<()> {
    let previous = read_clipboard();

    write_clipboard(text)?;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    info!("Windows clipboard paste: {} chars → Ctrl+V", text.len());
    send_key(VK_V, true)?;

    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if let Some(prev) = previous {
        let _ = write_clipboard(&prev);
    }
    Ok(())
}

/// Direct insertion is not available without UIA — fall through to clipboard.
pub async fn insert_text_direct(_text: &str) -> Result<()> {
    Err(anyhow!(
        "INSERTION_UNSUPPORTED: use clipboard fallback on Windows"
    ))
}

/// Delete `delete_chars` characters (Backspace × N) then paste `insert` via Ctrl+V.
/// Used for live streaming of partial transcripts into the focused field.
pub fn apply_text_edit(delete_chars: usize, insert: &str) -> Result<()> {
    if delete_chars == 0 && insert.is_empty() {
        return Ok(());
    }
    if !insert.is_empty() {
        write_clipboard(insert)?;
    }
    for _ in 0..delete_chars {
        send_key(VK_BACK, false)?;
    }
    if !insert.is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(
            if delete_chars == 0 { 20 } else { 30 },
        ));
        send_key(VK_V, true)?;
    }
    Ok(())
}

// ── Hold-to-talk key-up detection (WH_KEYBOARD_LL low-level hook) ─────────────
//
// tauri-plugin-global-shortcut may not deliver WM_KEYUP while another app has
// focus. WH_KEYBOARD_LL fires for all key events system-wide, regardless of focus.

static RELEASE_CB: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static RELEASE_ARMED: AtomicBool = AtomicBool::new(false);
static RELEASE_VK: AtomicU32 = AtomicU32::new(u32::MAX);
static RELEASE_NEED_CTRL: AtomicBool = AtomicBool::new(false);
static RELEASE_NEED_ALT: AtomicBool = AtomicBool::new(false);
static RELEASE_NEED_SHIFT: AtomicBool = AtomicBool::new(false);

static HOOK_HANDLE: OnceLock<isize> = OnceLock::new();

pub fn install_release_watch(on_release: impl Fn() + Send + Sync + 'static) {
    let _ = RELEASE_CB.set(Box::new(on_release));
    install_hook();
}

pub fn arm_release_watch(shortcut: &str) {
    let (vk, need_ctrl, need_alt, need_shift) = parse_shortcut_vk(shortcut);
    RELEASE_VK.store(vk, Ordering::Relaxed);
    RELEASE_NEED_CTRL.store(need_ctrl, Ordering::Relaxed);
    RELEASE_NEED_ALT.store(need_alt, Ordering::Relaxed);
    RELEASE_NEED_SHIFT.store(need_shift, Ordering::Relaxed);
    RELEASE_ARMED.store(true, Ordering::SeqCst);
    info!("Windows: watching release of {shortcut} (vk={vk:#x})");
}

pub fn clear_release_watch() {
    RELEASE_ARMED.store(false, Ordering::SeqCst);
}

fn install_hook() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        std::thread::Builder::new()
            .name("voicekey-kbhook".into())
            .spawn(|| unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
                };

                match SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0) {
                    Ok(h) => {
                        let _ = HOOK_HANDLE.set(h.0 as isize);
                        info!("Windows low-level keyboard hook installed");
                    }
                    Err(e) => {
                        warn!("Could not install keyboard hook: {e}");
                        return;
                    }
                }

                // Pump messages — required for WH_KEYBOARD_LL to receive events.
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .expect("failed to spawn keyboard hook thread");
    });
}

unsafe extern "system" fn ll_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 {
        let msg = w_param.0 as u32;
        if msg == WM_KEYUP || msg == WM_SYSKEYUP {
            let info = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
            note_release(info.vkCode);
        }
    }
    let hook = HOOK_HANDLE
        .get()
        .map(|&h| HHOOK(h as *mut _))
        .unwrap_or(HHOOK(std::ptr::null_mut()));
    CallNextHookEx(hook, n_code, w_param, l_param)
}

fn note_release(vk: u32) {
    if !RELEASE_ARMED.load(Ordering::SeqCst) {
        return;
    }
    let expected = RELEASE_VK.load(Ordering::Relaxed);

    if vk == expected {
        // The main key was released → fire immediately.
    } else if is_modifier_vk(vk) {
        // A modifier was released — only fire if it was part of the shortcut.
        let ctrl_rel = vk == 0x11 && RELEASE_NEED_CTRL.load(Ordering::Relaxed);
        let alt_rel = vk == 0x12 && RELEASE_NEED_ALT.load(Ordering::Relaxed);
        let shift_rel = vk == 0x10 && RELEASE_NEED_SHIFT.load(Ordering::Relaxed);
        if !ctrl_rel && !alt_rel && !shift_rel {
            return;
        }
    } else {
        return;
    }

    if !RELEASE_ARMED.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(cb) = RELEASE_CB.get() {
        cb();
    }
}

fn is_modifier_vk(vk: u32) -> bool {
    // VK_SHIFT=0x10, VK_CONTROL=0x11, VK_MENU(Alt)=0x12,
    // VK_LSHIFT=0xA0, VK_RSHIFT=0xA1, VK_LCONTROL=0xA2, VK_RCONTROL=0xA3,
    // VK_LMENU=0xA4, VK_RMENU=0xA5
    matches!(vk, 0x10 | 0x11 | 0x12 | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5)
}

fn parse_shortcut_vk(shortcut: &str) -> (u32, bool, bool, bool) {
    let mut need_ctrl = false;
    let mut need_alt = false;
    let mut need_shift = false;
    let mut vk = u32::MAX;
    for part in shortcut.split('+') {
        match part.trim() {
            "Ctrl" | "Control" => need_ctrl = true,
            "Alt" | "Option" => need_alt = true,
            "Shift" => need_shift = true,
            "Super" | "Win" | "Meta" => {}
            other => {
                if let Some(code) = windows_vk(other) {
                    vk = code;
                }
            }
        }
    }
    (vk, need_ctrl, need_alt, need_shift)
}

fn windows_vk(name: &str) -> Option<u32> {
    Some(match name {
        "A" => 0x41, "B" => 0x42, "C" => 0x43, "D" => 0x44,
        "E" => 0x45, "F" => 0x46, "G" => 0x47, "H" => 0x48,
        "I" => 0x49, "J" => 0x4A, "K" => 0x4B, "L" => 0x4C,
        "M" => 0x4D, "N" => 0x4E, "O" => 0x4F, "P" => 0x50,
        "Q" => 0x51, "R" => 0x52, "S" => 0x53, "T" => 0x54,
        "U" => 0x55, "V" => 0x56, "W" => 0x57, "X" => 0x58,
        "Y" => 0x59, "Z" => 0x5A,
        "0" => 0x30, "1" => 0x31, "2" => 0x32, "3" => 0x33,
        "4" => 0x34, "5" => 0x35, "6" => 0x36, "7" => 0x37,
        "8" => 0x38, "9" => 0x39,
        "Space" => 0x20,
        "Return" | "Enter" => 0x0D,
        "Tab" => 0x09,
        "Escape" | "Esc" => 0x1B,
        "Backspace" => 0x08,
        "Delete" => 0x2E,
        "Insert" => 0x2D,
        "Home" => 0x24, "End" => 0x23,
        "PageUp" => 0x21, "PageDown" => 0x22,
        "ArrowLeft" | "Left" => 0x25,
        "ArrowUp" | "Up" => 0x26,
        "ArrowRight" | "Right" => 0x27,
        "ArrowDown" | "Down" => 0x28,
        "Equal" => 0xBB,
        "Minus" => 0xBD,
        "F1" => 0x70, "F2" => 0x71, "F3" => 0x72, "F4" => 0x73,
        "F5" => 0x74, "F6" => 0x75, "F7" => 0x76, "F8" => 0x77,
        "F9" => 0x78, "F10" => 0x79, "F11" => 0x7A, "F12" => 0x7B,
        _ => return None,
    })
}

// ── Tauri commands ────────────────────────────────────────────────────────────

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
