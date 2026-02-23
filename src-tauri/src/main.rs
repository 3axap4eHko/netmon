// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod db;
mod mtr;
mod ping;
mod tray;
mod types;

use std::sync::{Arc, Mutex};
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt;

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
            // Focus existing window on second instance
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_targets,
            commands::add_target,
            commands::remove_target,
            commands::get_dashboard,
            commands::get_live_stats,
            commands::pause_monitoring,
            commands::resume_monitoring,
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

            // -- MTR Engine --
            let engine = mtr::MtrEngine::new(Arc::clone(&db), pinger);

            // Start monitoring active targets
            let active_targets = {
                let db_lock = db.lock().unwrap();
                db_lock
                    .get_active_targets()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.address)
                    .collect::<Vec<_>>()
            };
            engine.start(active_targets);

            // -- Manage state for commands --
            app.manage(Arc::clone(&db));
            app.manage(Arc::clone(&engine));

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
                .on_menu_event({
                    let app_handle = app.handle().clone();
                    move |_tray, event| {
                        match event.id().as_ref() {
                            "open" | "add_target" => {
                                if let Some(window) =
                                    app_handle.get_webview_window("main")
                                {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
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
                            if let Some(window) =
                                app_handle.get_webview_window("main")
                            {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // -- Window close → hide to tray --
            if let Some(window) = app.get_webview_window("main") {
                window.on_window_event({
                    let window = window.clone();
                    move |event| {
                        if let WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = window.hide();
                        }
                    }
                });
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
