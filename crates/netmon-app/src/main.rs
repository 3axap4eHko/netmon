// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod cloud_commands;
mod commands;
mod db;
mod http_probe;
mod load_test;
mod mtr;
mod ping;
mod sync;
mod tray;
mod types;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Listener, Manager, PhysicalPosition, PhysicalSize, RunEvent, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;

type GeoState = Arc<Mutex<Option<WindowGeometry>>>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct WindowGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

fn window_json_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join("window.json"))
}

fn load_window_geometry(path: &Path) -> Option<WindowGeometry> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn capture_geometry(window: &WebviewWindow) -> Option<WindowGeometry> {
    let maximized = window.is_maximized().unwrap_or(false);
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(WindowGeometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        maximized,
    })
}

fn apply_geometry(window: &WebviewWindow, geo: &WindowGeometry) {
    let _ = window.set_size(PhysicalSize::new(geo.width, geo.height));
    let _ = window.set_position(PhysicalPosition::new(geo.x, geo.y));
    if geo.maximized {
        let _ = window.maximize();
    }
}

fn attach_close_handler(window: &WebviewWindow, app: &tauri::AppHandle) {
    let app = app.clone();
    let window_ref = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
            // Let the window be destroyed (no prevent_close) so the webview frees
            // its resources; the process stays alive via RunEvent::ExitRequested.
            if let Some(geo) = capture_geometry(&window_ref) {
                if let Some(state) = app.try_state::<GeoState>() {
                    if let Ok(mut guard) = state.lock() {
                        *guard = Some(geo);
                    }
                }
                if let Some(path) = window_json_path(&app) {
                    match serde_json::to_vec(&geo) {
                        Ok(json) => {
                            if let Err(e) = std::fs::write(&path, json) {
                                eprintln!("Failed to persist window geometry: {}", e);
                            }
                        }
                        Err(e) => eprintln!("Failed to serialize window geometry: {}", e),
                    }
                }
            }
        }
    });
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    let geo = app
        .try_state::<GeoState>()
        .and_then(|state| state.lock().ok().and_then(|guard| *guard));

    let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("NetMon — Network Monitor")
        .min_inner_size(800.0, 500.0)
        .resizable(true)
        .decorations(true);
    let builder = match geo {
        Some(g) => builder
            .inner_size(g.width as f64, g.height as f64)
            .position(g.x as f64, g.y as f64),
        None => builder.inner_size(1100.0, 750.0),
    };

    match builder.build() {
        Ok(window) => {
            if geo.map(|g| g.maximized).unwrap_or(false) {
                let _ = window.maximize();
            }
            attach_close_handler(&window, app);
        }
        Err(e) => eprintln!("Failed to recreate main window: {}", e),
    }
}

fn load_icon(png_bytes: &[u8]) -> Image<'static> {
    // Decode PNG to raw RGBA
    let img = image::load_from_memory(png_bytes).expect("Failed to decode tray icon PNG");
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    Image::new_owned(rgba.into_raw(), w, h)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Re-open (or focus) the window on second instance
            show_main_window(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_targets,
            commands::add_target,
            commands::set_probe_mode,
            commands::remove_target,
            commands::get_dashboard,
            commands::get_live_stats,
            commands::pause_monitoring,
            commands::resume_monitoring,
            commands::get_monitoring_paused,
            commands::run_load_test,
            commands::get_load_test_history,
            commands::get_ui_settings,
            commands::set_ui_settings,
            cloud_commands::get_auth_state,
            cloud_commands::start_oauth,
            cloud_commands::login_email,
            cloud_commands::register_email,
            cloud_commands::logout,
            cloud_commands::get_sync_status,
            cloud_commands::get_account_info,
        ])
        .setup(|app| {
            // -- Database --
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let db_path = app_data.join("data.db");

            // Data migration: check for existing Electron DB
            if let Some(appdata) = std::env::var_os("APPDATA") {
                let electron_path =
                    std::path::PathBuf::from(appdata).join("netmon").join("data.db");
                if electron_path.exists() && !db_path.exists() {
                    eprintln!("Migrating Electron database from {:?}", electron_path);
                    if let Err(e) = std::fs::copy(&electron_path, &db_path) {
                        eprintln!("Failed to copy Electron DB: {}", e);
                    }
                }
            }

            let database = db::Database::open(&db_path).expect("Failed to open database");

            // If we migrated from Electron, ensure new tables exist
            database.migrate_from_electron().ok();

            let db = Arc::new(Mutex::new(database));

            // -- ICMP Pinger --
            let pinger = ping::IcmpPinger::new().expect("Failed to create ICMP pinger");

            // -- HTTP Prober --
            let http_prober = http_probe::HttpProber::new().expect("Failed to create HTTP prober");

            // -- MTR Engine --
            let engine = mtr::MtrEngine::new(Arc::clone(&db), pinger, http_prober);

            // Start monitoring active targets
            let active_targets = {
                let db_lock = db.lock().unwrap();
                db_lock
                    .get_active_targets()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| (t.address, t.probe_mode))
                    .collect::<Vec<_>>()
            };
            engine.start(active_targets);

            // -- Load Test Engine --
            let load_test_engine = Arc::new(load_test::LoadTestEngine::new(Arc::clone(&db)));

            // -- Auth & Sync --
            let auth_manager = auth::AuthManager::new(Arc::clone(&db));
            let sync_engine = sync::SyncEngine::new(Arc::clone(&db), Arc::clone(&auth_manager));

            // Auto-start sync if already authenticated
            if auth_manager.get_auth_state().authenticated {
                sync_engine.start();
            }

            // -- Manage state for commands --
            app.manage(Arc::clone(&db));
            app.manage(Arc::clone(&engine));
            app.manage(Arc::clone(&load_test_engine));
            app.manage(Arc::clone(&auth_manager));
            app.manage(Arc::clone(&sync_engine));

            // -- Persisted window geometry (restored across tray cycles and restarts) --
            let initial_geo = window_json_path(&app.handle()).and_then(|path| load_window_geometry(&path));
            let geo_state: GeoState = Arc::new(Mutex::new(initial_geo));
            app.manage(Arc::clone(&geo_state));

            // -- Deep link handler --
            let auth_for_deeplink = Arc::clone(&auth_manager);
            let sync_for_deeplink = Arc::clone(&sync_engine);
            app.listen("deep-link://new-url", move |event: tauri::Event| {
                let url = event.payload().to_string();
                // Parse netmon://auth/callback?code=XXX
                if let Some(code) = extract_auth_code(&url) {
                    match auth_for_deeplink.handle_callback(&code) {
                        Ok(state) => {
                            if state.authenticated {
                                sync_for_deeplink.start();
                            }
                            eprintln!("OAuth callback successful");
                        }
                        Err(e) => eprintln!("OAuth callback failed: {}", e),
                    }
                }
            });

            // -- System Tray --
            let icon_bytes = tray::create_tray_icon(tray::TrayColor::Green);
            let icon = load_icon(&icon_bytes);

            let open_item =
                MenuItemBuilder::with_id("open", "Open Dashboard").build(app)?;
            let add_target_item =
                MenuItemBuilder::with_id("add_target", "Add Target...").build(app)?;
            let pause_item =
                MenuItemBuilder::with_id("pause", "Pause Monitoring").build(app)?;
            let resume_item =
                MenuItemBuilder::with_id("resume", "Resume Monitoring").build(app)?;

            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let autostart_item =
                CheckMenuItemBuilder::with_id("autostart", "Start with Windows")
                    .checked(autostart_enabled)
                    .build(app)?;

            let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

            let menu = MenuBuilder::new(app)
                .item(&open_item)
                .separator()
                .item(&add_target_item)
                .separator()
                .item(&pause_item)
                .item(&resume_item)
                .separator()
                .item(&autostart_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(icon)
                .tooltip("NetMon — Network Monitor")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event({
                    let app_handle = app.handle().clone();
                    move |_tray, event| {
                        match event.id().as_ref() {
                            "open" | "add_target" => {
                                show_main_window(&app_handle);
                            }
                            "pause" => {
                                if let Some(engine) =
                                    app_handle.try_state::<Arc<mtr::MtrEngine>>()
                                {
                                    engine.pause();
                                }
                            }
                            "resume" => {
                                if let Some(engine) =
                                    app_handle.try_state::<Arc<mtr::MtrEngine>>()
                                {
                                    engine.resume();
                                }
                            }
                            "autostart" => {
                                let manager = app_handle.autolaunch();
                                let enabled = manager.is_enabled().unwrap_or(false);
                                if enabled {
                                    let _ = manager.disable();
                                } else {
                                    let _ = manager.enable();
                                }
                            }
                            "quit" => {
                                std::process::exit(0);
                            }
                            _ => {}
                        }
                    }
                })
                .on_tray_icon_event({
                    let app_handle = app.handle().clone();
                    move |_tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(&app_handle);
                        }
                    }
                })
                .build(app)?;

            // -- Window close → destroy webview (frees rendering resources) --
            if let Some(window) = app.get_webview_window("main") {
                if let Some(geo) = initial_geo {
                    apply_geometry(&window, &geo);
                }
                attach_close_handler(&window, &app.handle());
            }

            // -- Background timers --

            // Tray color update every 10 seconds
            let db_tray = Arc::clone(&db);
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut last_color = tray::TrayColor::Green;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    let max_loss = tray::get_current_max_loss(&db_tray);
                    let new_color = tray::color_from_loss(max_loss);
                    if new_color != last_color {
                        last_color = new_color;
                        let icon_bytes = tray::create_tray_icon(new_color);
                        let icon = load_icon(&icon_bytes);
                        if let Some(tray) = app_handle.tray_by_id("main-tray") {
                            let _ = tray.set_icon(Some(icon));
                        }
                    }
                }
            });

            // Aggregation every 5 minutes
            let db_agg = Arc::clone(&db);
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(300));
                    if let Ok(db) = db_agg.lock() {
                        if let Err(e) = db.run_aggregation() {
                            eprintln!("Aggregation error: {}", e);
                        }
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            // Keep the process (and monitoring threads) alive after the window is
            // closed to the tray; only the explicit Quit menu exits via process::exit.
            if let RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

fn extract_auth_code(url: &str) -> Option<String> {
    let url = url.trim_matches('"');
    if let Some(query_start) = url.find('?') {
        let query = &url[query_start + 1..];
        for pair in query.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                if key == "code" {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}
