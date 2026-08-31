//! App wiring: plugins, commands, tray, and startup placement.
//!
//! Stage transitions live in [`notch_daemon`]; this module only mutates
//! [`notch_daemon::Shared`] and asks it to apply.

mod app_icons;
mod commands;
mod db;
mod media;
mod notch_daemon;
mod sound;
mod tracker;
mod windows;

use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use tauri_plugin_notification::init as notification_plugin;

use notch_daemon::{SharedHandle, Stage};

/// Recursively copies a directory tree (used by the legacy-data migration
/// when a same-volume move is not possible).
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Tray toggle: Hide parks the widget; Show restores the compact pill
/// on the desktop layer.
fn toggle_main(app: &tauri::AppHandle, shared: &SharedHandle) {    let to_hidden = {
        let mut s = match shared.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        let to_hidden = s.stage != Stage::Hidden;
        s.stage = if to_hidden {
            Stage::Hidden
        } else {
            Stage::Compact
        };
        if to_hidden {
            s.pin = false;
            s.collapse_requested = true;
        }
        to_hidden
    };

    match to_hidden {
        true => {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.hide();
            }
        }
        false => {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                windows::place_main(&win);
            }
            notch_daemon::apply_stage(app, Stage::Compact);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second app launch is redirected here
        // and simply surfaces the running widget.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                windows::place_main(&win);
            }
            notch_daemon::apply_stage(app, Stage::Compact);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(notification_plugin())
        .invoke_handler(tauri::generate_handler![
            commands::expand_now,
            commands::collapse_now,
            commands::set_pin,
            commands::set_island_size,
            commands::log_event,
            commands::get_stats,
            commands::recent_apps,
            commands::get_config,
            commands::save_config,
            commands::add_task,
            commands::list_tasks,
            commands::set_task_done,
            commands::set_task_duration,
            commands::update_task,
            commands::delete_task,
            commands::get_app_icon,
            commands::notify_focus_done,
            commands::autostart_enabled,
            commands::autostart_set,
            media::media_play,
            media::media_pause,
            media::media_next,
            media::media_prev,
            media::media_seek
        ])
        .setup(|app| {
            // Rebrand migration: the first releases stored data under the
            // `com.yado.activitywidget` identifier. Move it over so users
            // keep their history, config and app-icon cache.
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            if let Some(parent) = data_dir.parent() {
                let legacy = parent.join("com.yado.activitywidget");
                if !data_dir.exists() && legacy.exists() {
                    if std::fs::rename(&legacy, &data_dir).is_err() {
                        let _ = copy_dir_recursive(&legacy, &data_dir);
                    }
                    eprintln!("[migrate] legacy data moved to {}", data_dir.display());
                }
            }
            std::fs::create_dir_all(&data_dir).expect("failed to create data dir");

            // Launch at login so the notch is always on (user-disableable
            // from the settings page).
            {
                use tauri_plugin_autostart::ManagerExt;
                if let Err(e) = app.autolaunch().enable() {
                    eprintln!("[autostart] enable failed: {e}");
                }
            }
            let conn = db::init(&data_dir.join("activities.db"))
                .expect("failed to initialize sqlite database");
            app.manage(db::Db(Mutex::new(conn)));
            app_icons::init(&data_dir);
            app.manage(media::MediaHandle::default());

            let shared: SharedHandle = Arc::new(Mutex::new(notch_daemon::Shared {
                stage: Stage::Compact,
                pin: false,
                collapse_requested: false,
                island_w: windows::ISLAND_DASHBOARD_SIZE.0,
                island_h: windows::ISLAND_DASHBOARD_SIZE.1,
            }));
            app.manage(shared.clone());

            tracker::spawn_focus_tracker(data_dir.clone());
            tracker::spawn_file_watcher(data_dir.clone());
            media::spawn(app.handle().clone());

            let main = app
                .get_webview_window("main")
                .expect("main window must exist");
            windows::place_main(&main);
            main.show()?;
            let _ = main.set_skip_taskbar(true);
            let _ = main.set_ignore_cursor_events(true);
            #[cfg(windows)]
            {
                use windows::z_order;
                if let Ok(hwnd) = main.hwnd() {
                    z_order::force_skip_taskbar(hwnd.0 as isize);
                    z_order::set_z_order(hwnd.0 as isize, true);
                }
            }

            notch_daemon::spawn(app.handle().clone(), shared.clone());

            let show_hide =
                MenuItem::with_id(app, "showhide", "Show / Hide", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_hide, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("no default icon").clone())
                .tooltip("Yado Notch")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event({
                    let shared = shared.clone();
                    move |app, event| match event.id.as_ref() {
                        "showhide" => toggle_main(app, &shared),
                        "quit" => app.exit(0),
                        _ => {}
                    }
                })
                .on_tray_icon_event({
                    let shared = shared.clone();
                    move |tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            toggle_main(tray.app_handle(), &shared);
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
