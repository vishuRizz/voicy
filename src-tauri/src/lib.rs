// VoiceKey – lib.rs
// Library crate root (required by Tauri 2's mobile-compatible architecture).

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

pub mod app_state;
pub mod audio;
pub mod errors;
pub mod hotkey;
pub mod insertion;
pub mod platform;
pub mod settings;
pub mod transcription;

use app_state::AppState;
use settings::Settings;
use tauri::Manager;
use tracing::info;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing (logs to stderr; no user data logged).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voicekey=info,warn".parse().unwrap()),
        )
        .init();

    info!("Voicy starting…");

    // Set the process display name early so macOS shows "Voicy" in
    // System Settings → Privacy lists (Microphone, Accessibility).
    #[cfg(target_os = "macos")]
    platform::set_process_name();

    tauri::Builder::default()
        // ── plugins ───────────────────────────────────────────────────────
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        // ── managed state ─────────────────────────────────────────────────
        .manage(AppState::new_shared())
        .manage(parking_lot::RwLock::new(Settings::default()))
        // Cell for passing the stop-sender from key-down to key-up handler.
        // Wrapped in Arc so the timer task can clone a reference to it.
        .manage(std::sync::Arc::new(parking_lot::Mutex::new(
            None::<tokio::sync::oneshot::Sender<bool>>,
        )))
        // Lets key-up stop the microphone immediately, even if Whisper is busy.
        .manage(std::sync::Arc::new(parking_lot::Mutex::new(
            None::<std::sync::mpsc::Sender<()>>,
        )))
        // ── tray icon ─────────────────────────────────────────────────────
        .setup(|app| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

            TrayIconBuilder::new()
                .tooltip("Voicy — hold to dictate")
                .icon(app.default_window_icon().cloned().expect("app icon missing"))
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    else {
                        return;
                    };
                    commands::toggle_menu(tray.app_handle(), rect);
                })
                .build(app)?;

            Ok(())
        })
        // ── commands ──────────────────────────────────────────────────────
        .invoke_handler(tauri::generate_handler![
            settings::commands::get_settings,
            settings::commands::update_settings,
            transcription::cancel_session,
            platform::get_permission_status,
            platform::request_microphone_permission,
            platform::request_accessibility_permission,
            commands::get_model_status,
            commands::download_model,
            commands::start_onboarding_check,
            commands::open_settings_window,
            commands::quit_app,
            commands::open_portfolio,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Voicy")
        .run(|app_handle, event| {
            // Register only once the event loop is running. Doing it from
            // setup (before the loop pumps main-thread tasks) installs a
            // shortcut that never fires until the user hits Save, which
            // re-registers after the loop is up.
            if let tauri::RunEvent::Ready = event {
                register_startup_shortcut(app_handle);
            }
        });
}

fn register_startup_shortcut(app: &tauri::AppHandle) {
    let saved = match settings::commands::load_settings(app) {
        Ok(settings) => settings,
        Err(e) => {
            tracing::warn!("Could not load settings ({e}), using defaults");
            Settings::default()
        }
    };

    let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
    *settings_state.write() = saved.clone();

    match hotkey::register_shortcut(app, &saved) {
        Ok(()) => {
            let _ = tauri::Emitter::emit(app, "voicekey://hotkey-ok", &saved.shortcut);
        }
        Err(e) => {
            tracing::error!("Hotkey registration failed on startup: {e}");
            let _ = tauri::Emitter::emit(app, "voicekey://hotkey-failed", &e.to_string());
        }
    }
}

/// Miscellaneous top-level Tauri commands.
pub mod commands {
    use crate::settings::Settings;
    use serde::Serialize;
    use tauri::{AppHandle, Emitter, Manager};

    #[derive(Serialize)]
    pub struct ModelStatus {
        pub model: String,
        pub installed: bool,
        pub path: Option<String>,
        pub size_mb: u32,
    }

    /// Return whether a model file is present.
    ///
    /// `model` is the quality the settings screen just selected. When it is
    /// omitted, the saved setting is checked instead.
    #[tauri::command]
    pub async fn get_model_status(
        app: AppHandle,
        model: Option<String>,
        language: Option<String>,
    ) -> Result<ModelStatus, String> {
        use crate::settings::WhisperModel;

        let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
        let saved = settings_state.read().clone();
        let selected = match model.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => WhisperModel::parse(name)?,
            None => saved.model.clone(),
        };
        let language = language.unwrap_or(saved.language);
        let (model, size_mb, filename) = (
            format!("{:?}", selected).to_lowercase(),
            selected.size_mb_for(&language),
            selected.filename_for(&language),
        );

        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?;
        let path = resource_dir.join("models").join(&filename);
        let installed = path.exists();

        Ok(ModelStatus {
            model,
            installed,
            path: if installed {
                path.to_str().map(|s| s.to_string())
            } else {
                None
            },
            size_mb,
        })
    }

    /// Run the onboarding permission check flow.
    #[tauri::command]
    pub async fn start_onboarding_check(app: AppHandle) -> Result<(), String> {
        let status = crate::platform::check_permissions().await;
        let _ = Emitter::emit(&app, "permissions://status", &status);
        Ok(())
    }
    /// Stream-download a Whisper model file and emit progress events.
    ///
    /// Emits `voicekey://download-progress` with `{ pct: f32, done: bool, error: Option<String> }`
    #[tauri::command]
    pub async fn download_model(app: AppHandle) -> Result<(), String> {
        use std::io::{Read, Write};

        let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
        let (filename, size_mb) = {
            let s = settings_state.read();
            (
                s.model.filename_for(&s.language).to_string(),
                s.model.size_mb_for(&s.language),
            )
        };

        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|e| e.to_string())?;
        let models_dir = resource_dir.join("models");
        std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;
        let dest = models_dir.join(&filename);

        // Base URL for whisper.cpp GGML models on Hugging Face.
        let url = format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}"
        );

        tracing::info!("Downloading model: {url} → {}", dest.display());

        #[derive(serde::Serialize, Clone)]
        struct Progress { pct: f32, done: bool, error: Option<String> }

        let app_for_emit = app.clone();
        let emit = std::sync::Arc::new(move |pct: f32, done: bool, error: Option<String>| {
            let _ = Emitter::emit(&app_for_emit, "voicekey://download-progress",
                &Progress { pct, done, error });
        });

        emit(0.0, false, None);

        // Use a blocking thread so we can stream without async complexity.
        let dest_clone = dest.clone();
        let total_bytes = size_mb as u64 * 1024 * 1024;
        let emit_inner = emit.clone();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let resp = ureq::get(&url)
                .call()
                .map_err(|e| format!("Download failed: {e}"))?;

            let mut file = std::fs::File::create(&dest_clone)
                .map_err(|e| format!("Cannot create file: {e}"))?;

            let mut reader = resp.into_reader();
            let mut buf = vec![0u8; 65_536]; // 64 KB chunks
            let mut downloaded: u64 = 0;

            loop {
                let n = reader.read(&mut buf).map_err(|e| format!("Read error: {e}"))?;
                if n == 0 { break; }
                file.write_all(&buf[..n]).map_err(|e| format!("Write error: {e}"))?;
                downloaded += n as u64;
                let pct = if total_bytes > 0 {
                    (downloaded as f32 / total_bytes as f32 * 100.0).min(99.0)
                } else { 50.0 };
                emit_inner(pct, false, None);
            }

            Ok(())
        })
        .await
        .map_err(|e| format!("Task failed: {e}"))
        .and_then(|r| r)
        .map_err(|e| {
            emit(0.0, false, Some(e.clone()));
            e
        })?;

        emit(100.0, true, None);
        tracing::info!("Model downloaded: {}", dest.display());
        Ok(())
    }

    /// Show the main window. `view` is `settings` or `permissions`.
    #[tauri::command]
    pub fn open_settings_window(app: AppHandle, view: Option<String>) {
        if let Some(menu) = app.get_webview_window("menu") {
            let _ = menu.hide();
        }
        if let Some(view) = view {
            let _ = Emitter::emit(&app, "voicekey://show-view", view);
        }
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.show();
            let _ = win.unminimize();
            let _ = win.set_focus();
        }
    }

    #[tauri::command]
    pub fn quit_app(app: AppHandle) {
        app.exit(0);
    }

    /// Open the portfolio in the default browser.
    #[tauri::command]
    pub fn open_portfolio() {
        let url = "https://vishu.app";
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(url).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }

    /// Pop the menu-bar panel under the status icon, or hide it if it is open.
    pub fn toggle_menu(app: &AppHandle, rect: tauri::Rect) {
        let Some(win) = app.get_webview_window("menu") else {
            return;
        };
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
            return;
        }
        if let (Ok(scale), Ok(size)) = (win.scale_factor(), win.outer_size()) {
            let scale = if scale == 0.0 { 1.0 } else { scale };
            let icon = rect.position.to_logical::<f64>(scale);
            let icon_size = rect.size.to_logical::<f64>(scale);
            let width = size.to_logical::<f64>(scale).width;
            let x = (icon.x + icon_size.width - width).max(8.0);
            let y = icon.y + icon_size.height + 6.0;
            let _ = win.set_position(tauri::LogicalPosition::new(x, y));
        }
        let _ = win.show();
        let _ = win.set_focus();
    }
}
