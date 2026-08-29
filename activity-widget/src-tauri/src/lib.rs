use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    LogicalSize, Manager, PhysicalPosition, PhysicalSize, State,
};

mod db;
mod tracker;

#[tauri::command]
fn log_event(state: State<db::Db>, app_name: String, event_type: String, detail: Option<String>) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    db::insert(&conn, &app_name, &event_type, detail.as_deref(), ts).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_stats(state: State<db::Db>) -> Result<db::Stats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::get_stats(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn recent_apps(state: State<db::Db>) -> Result<Vec<db::AppCount>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::recent_apps(&conn, 24).map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
struct SaveConfigArgs {
    mode: String,
    apps: Vec<String>,
    watch_folders: Option<Vec<String>>,
}

#[tauri::command]
fn get_config(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let path = tracker::config_path(&data_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        Err(_) => serde_json::to_value(tracker::TrackerConfig::default()).map_err(|e| e.to_string()),
    }
}

#[tauri::command]
fn save_config(app: tauri::AppHandle, args: SaveConfigArgs) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;
    let cfg = tracker::TrackerConfig {
        mode: match args.mode.as_str() {
            "allowlist" => tracker::AppFilterMode::Allowlist,
            _ => tracker::AppFilterMode::All,
        },
        apps: args.apps,
        watch_folders: args.watch_folders,
    };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    std::fs::write(tracker::config_path(&data_dir), json).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_task(
    state: State<db::Db>,
    title: String,
    notes: Option<String>,
    duration_min: Option<i64>,
) -> Result<db::Task, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    db::add_task(&conn, &title, notes.as_deref(), duration_min, ts).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_tasks(state: State<db::Db>) -> Result<Vec<db::Task>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_tasks(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_task_done(state: State<db::Db>, id: i64, done: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    db::set_task_done(&conn, id, done, ts)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_task(state: State<db::Db>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_task(&conn, id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

const CLOSED_SIZE: (f64, f64) = (320.0, 42.0);
const OPEN_SIZE: (f64, f64) = (400.0, 322.0);
const TOP_MARGIN: f64 = 0.0;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn set_widget_state(app: tauri::AppHandle, expanded: bool) {
    if let Some(win) = app.get_webview_window("main") {
        let (w, h) = if expanded { OPEN_SIZE } else { CLOSED_SIZE };
        let _ = win.set_size(LogicalSize::new(w, h));
        place_top_center(&win);
        #[cfg(windows)]
        {
            if let Ok(hwnd) = win.hwnd() {
                win_ex::set_bottom(hwnd.0 as isize);
            }
        }
    }
}

fn place_top_center(win: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = win.primary_monitor() else {
        return;
    };
    let size = win
        .outer_size()
        .unwrap_or(PhysicalSize::new(320, 48));
    let scale = monitor.scale_factor();
    let m = monitor.size();
    let margin = (TOP_MARGIN * scale).round() as i32;
    let x = ((m.width as f64 - size.width as f64) / 2.0).round() as i32;
    let _ = win.set_position(PhysicalPosition::new(x, margin));
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
        }
    }
}

#[cfg(windows)]
mod win_ex {
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const SW_HIDE: i32 = 0;
    const SW_SHOWNA: i32 = 8;
    const HWND_BOTTOM: isize = 1;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_NOOWNERZORDER: u32 = 0x0200;

    #[link(name = "user32")]
    extern "system" {
        fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
        fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
        fn SetWindowPos(
            hwnd: isize,
            after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }

    pub fn force_skip_taskbar(hwnd: isize) {
        unsafe {
            let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                (cur & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW,
            );
            ShowWindow(hwnd, SW_HIDE);
            ShowWindow(hwnd, SW_SHOWNA);
        }
    }

    pub fn set_bottom(hwnd: isize) {
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            set_widget_state,
            log_event,
            get_stats,
            recent_apps,
            get_config,
            save_config,
            add_task,
            list_tasks,
            set_task_done,
            delete_task
        ])
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to resolve app data dir");
            std::fs::create_dir_all(&data_dir).expect("failed to create data dir");
            let conn = db::init(&data_dir.join("activities.db"))
                .expect("failed to initialize sqlite database");

            // Storage self-test: insert, read back, remove.
            match db::insert(&conn, "SelfTest", "app_focus", Some("stage-3 verification"), 0) {
                Ok(_) => match db::count_all(&conn) {
                    Ok(n) => println!("[db self-test] write+read OK, rows={n}"),
                    Err(e) => println!("[db self-test] read failed: {e}"),
                },
                Err(e) => println!("[db self-test] write failed: {e}"),
            }
            let _ = db::delete_by_app_and_type(&conn, "SelfTest", "app_focus");
            println!(
                "[db self-test] cleaned up, rows={}",
                db::count_all(&conn).unwrap_or(-1)
            );

            app.manage(db::Db(std::sync::Mutex::new(conn)));

            tracker::spawn_focus_tracker(data_dir.clone());
            tracker::spawn_file_watcher(data_dir);

            let win = app
                .get_webview_window("main")
                .expect("main window must exist");

            place_top_center(&win);
            win.show()?;
            let _ = win.set_skip_taskbar(true);
            #[cfg(windows)]
            {
                let hwnd = win.hwnd().expect("failed to get hwnd").0 as isize;
                win_ex::force_skip_taskbar(hwnd);
                win_ex::set_bottom(hwnd);

                let pin_win = win.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(1500));
                    if pin_win.is_visible().unwrap_or(false) {
                        if let Ok(hwnd) = pin_win.hwnd() {
                            win_ex::set_bottom(hwnd.0 as isize);
                        }
                    }
                });
            }

            let show_hide =
                MenuItem::with_id(app, "showhide", "Show / Hide Widget", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_hide, &quit])?;

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().expect("no default icon").clone())
                .tooltip("Activity Widget")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "showhide" => toggle_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
