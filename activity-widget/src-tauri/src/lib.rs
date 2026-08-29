//! App wiring: plugins, commands, tray, and startup placement.
//!
//! Stage transitions live in [`notch_daemon`]; this module only mutates
//! [`notch_daemon::Shared`] and asks it to apply.

mod commands;
mod db;
mod notch_daemon;
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

/// Tray toggle: Hide parks the widget; Show restores the compact pill
/// on the desktop layer.
fn toggle_main(app: &tauri::AppHandle, shared: &SharedHandle) {
    let to_hidden = {
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
            commands::delete_task
        ])
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create data dir");
            let conn = db::init(&data_dir.join("activities.db"))
                .expect("failed to initialize sqlite database");
            app.manage(db::Db(Mutex::new(conn)));

            let shared: SharedHandle = Arc::new(Mutex::new(notch_daemon::Shared {
                stage: Stage::Compact,
                pin: false,
                collapse_requested: false,
                island_w: windows::ISLAND_DASHBOARD_SIZE.0,
                island_h: windows::ISLAND_DASHBOARD_SIZE.1,
            }));
            app.manage(shared.clone());

            tracker::spawn_focus_tracker(data_dir.clone());
            tracker::spawn_file_watcher(data_dir);

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
                .tooltip("Activity Widget")
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
