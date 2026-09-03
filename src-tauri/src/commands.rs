use tauri::{AppHandle, Manager, State};

use crate::notch_daemon::{self, SharedHandle};
use crate::{app_icons, db, tracker};

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------- stage control ----------

#[tauri::command]
pub fn expand_now(app: AppHandle, shared: State<'_, SharedHandle>) {
    notch_daemon::expand_now(&shared, &app);
}

#[tauri::command]
pub fn collapse_now(app: AppHandle, shared: State<'_, SharedHandle>) {
    notch_daemon::collapse_now(&shared, &app);
}

#[tauri::command]
pub fn set_pin(shared: State<'_, SharedHandle>, pin: bool) {
    if let Ok(mut s) = shared.lock() {
        s.pin = pin;
    }
}

/// Publishes the expanded island's live size (dashboard, settings or
/// tasks page) so the daemon's click-through and leave detection match
/// the visible surface. Values are logical pixels.
#[tauri::command]
pub fn set_island_size(shared: State<'_, SharedHandle>, width: f64, height: f64) {
    if let Ok(mut s) = shared.lock() {
        s.island_w = width.clamp(44.0, 600.0);
        s.island_h = height.clamp(44.0, 464.0);
    }
}

// ---------- events ----------

#[tauri::command]
pub fn log_event(
    state: State<'_, db::Db>,
    app_name: String,
    event_type: String,
    detail: Option<String>,
) -> Result<i64, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::insert(&conn, &app_name, &event_type, detail.as_deref(), now_ts())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stats(state: State<'_, db::Db>) -> Result<db::Stats, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::get_stats(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn recent_apps(state: State<'_, db::Db>) -> Result<Vec<db::AppCount>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::recent_apps(&conn, 24).map_err(|e| e.to_string())
}

// ---------- tracker config ----------

#[tauri::command]
pub fn get_config(app: AppHandle) -> Result<serde_json::Value, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    match std::fs::read_to_string(tracker::config_path(&data_dir)) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        Err(_) => {
            serde_json::to_value(tracker::TrackerConfig::default()).map_err(|e| e.to_string())
        }
    }
}

#[derive(serde::Deserialize)]
pub struct SaveConfigArgs {
    pub mode: String,
    pub apps: Vec<String>,
    pub watch_folders: Option<Vec<String>>,
}

#[tauri::command]
pub fn save_config(app: AppHandle, args: SaveConfigArgs) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
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

// ---------- tasks ----------

#[tauri::command]
pub fn add_task(
    state: State<'_, db::Db>,
    title: String,
    notes: Option<String>,
    due_date: Option<String>,
    duration_min: Option<i64>,
) -> Result<db::Task, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::add_task(
        &conn,
        &title,
        notes.as_deref(),
        due_date.as_deref(),
        duration_min,
        now_ts(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tasks(
    state: State<'_, db::Db>,
    filter: String,
    day: Option<String>,
) -> Result<Vec<db::Task>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::list_tasks(&conn, &filter, day.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_task_done(state: State<'_, db::Db>, id: i64, done: bool) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::set_task_done(&conn, id, done, now_ts())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_task_duration(
    state: State<'_, db::Db>,
    id: i64,
    duration_min: Option<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::set_task_duration(&conn, id, duration_min)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_task(
    state: State<'_, db::Db>,
    id: i64,
    title: String,
    notes: Option<String>,
    due_date: Option<String>,
    duration_min: Option<i64>,
) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::update_task(
        &conn,
        id,
        &title,
        notes.as_deref(),
        due_date.as_deref(),
        duration_min,
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_task(state: State<'_, db::Db>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_task(&conn, id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ---------- system ----------

#[tauri::command]
pub fn autostart_enabled(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let autostart = app.autolaunch();
    if enable {
        autostart.enable()
    } else {
        autostart.disable()
    }
    .map_err(|e| e.to_string())
}

// ---------- shelf ----------

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ShelfItem {
    pub path: String,
    pub name: String,
    pub added_at: u64,
}

fn shelf_file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("shelf.json"))
}

#[tauri::command]
pub fn shelf_load(app: AppHandle) -> Result<Vec<ShelfItem>, String> {
    let path = shelf_file(&app)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
        Err(_) => Ok(Vec::new()),
    }
}

#[tauri::command]
pub fn shelf_save(app: AppHandle, items: Vec<ShelfItem>) -> Result<(), String> {
    let path = shelf_file(&app)?;
    let json = serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// Reveals a path in the system file manager (selects it on Windows).
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        Err("unsupported platform".into())
    }
}

/// Starts a native OS drag of the given paths (blocks until drop/cancel).
/// Runs on a dedicated thread: OLE needs a fresh STA COM apartment, and
/// reusing pool threads silently fails when another task initialized COM
/// differently. Returns the effect the target chose: "move" | "copy" | "link" | "none".
#[tauri::command]
pub async fn start_file_drag(paths: Vec<String>) -> Result<String, String> {
    #[cfg(windows)]
    {
        if paths.is_empty() {
            return Err("no files to drag".into());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::shelf_drag::do_file_drag(&paths).map(|effect| match effect {
                crate::shelf_drag::DragEffect::Move => "move".into(),
                crate::shelf_drag::DragEffect::Copy => "copy".into(),
                crate::shelf_drag::DragEffect::Link => "link".into(),
                crate::shelf_drag::DragEffect::None => "none".into(),
            });
            eprintln!("[shelf] drag-out finished: {result:?}");
            let _ = tx.send(result);
        });
        rx.recv().map_err(|e| e.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = paths;
        Err("drag-out is only supported on Windows".into())
    }
}

// ---------- app icons ----------

#[tauri::command]
pub fn get_app_icon(app: String) -> Option<String> {
    app_icons::data_url(&app)
}

#[tauri::command]
pub fn notify_focus_done(app: AppHandle, title: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    eprintln!("[focus] session completed, notifying: {title}");
    crate::sound::notification_chime();
    match app
        .notification()
        .builder()
        .title("Focus session complete")
        .body(&title)
        .show()
    {
        Ok(_) => {
            eprintln!("[focus] toast shown");
            Ok(())
        }
        Err(e) => {
            // Toasts from unsigned dev builds are dropped by Windows; the
            // chime above is the reliable signal until the app is installed.
            eprintln!("[focus] toast not shown: {e}");
            Ok(())
        }
    }
}
