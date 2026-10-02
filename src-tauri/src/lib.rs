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

    info!("VoiceKey starting…");

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
        // ── setup ─────────────────────────────────────────────────────────
        .setup(|app| {
            // Load persisted settings and apply the saved shortcut.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match settings::commands::get_settings(handle.clone()).await {
                    Ok(saved) => {
                        let settings_state: tauri::State<parking_lot::RwLock<Settings>> =
                            handle.state();
                        *settings_state.write() = saved.clone();

                        if let Err(e) = hotkey::register_shortcut(&handle, &saved) {
                            tracing::error!("Hotkey registration failed on startup: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Could not load settings ({e}), using defaults");
                        let defaults = Settings::default();
                        if let Err(e) = hotkey::register_shortcut(&handle, &defaults) {
                            tracing::error!("Hotkey registration failed: {e}");
                        }
                    }
                }
            });

            Ok(())
        })
        // ── tray icon ─────────────────────────────────────────────────────
        .setup(|app| {
            use tauri::{
                menu::{Menu, MenuItem, PredefinedMenuItem},
                tray::TrayIconBuilder,
            };

            let settings_item =
                MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit VoiceKey", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_item, &sep, &quit_item])?;

            TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("VoiceKey — hold to dictate")
                .icon(app.default_window_icon().cloned().expect("app icon missing"))
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
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
            commands::start_onboarding_check,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VoiceKey");
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

    /// Return whether the selected model file is present.
    #[tauri::command]
    pub async fn get_model_status(app: AppHandle) -> Result<ModelStatus, String> {
        let settings_state: tauri::State<parking_lot::RwLock<Settings>> = app.state();
        let (model, size_mb, filename) = {
            let s = settings_state.read();
            (
                format!("{:?}", s.model).to_lowercase(),
                s.model.size_mb(),
                s.model.filename().to_string(),
            )
        };

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
}
