use tauri::{AppHandle, Manager, State};

use crate::notch_daemon::{self, SharedHandle};
use crate::{db, tracker};

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
        s.island_w = width.clamp(44.0, 560.0);
        s.island_h = height.clamp(44.0, 448.0);
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
pub fn delete_task(state: State<'_, db::Db>, id: i64) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    db::delete_task(&conn, id)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
