use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db(pub Mutex<Connection>);

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS events (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    app_name   TEXT NOT NULL,
    event_type TEXT NOT NULL,
    detail     TEXT,
    ts         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
CREATE INDEX IF NOT EXISTS idx_events_app ON events(app_name);
CREATE TABLE IF NOT EXISTS tasks (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    title        TEXT NOT NULL,
    notes        TEXT,
    duration_min INTEGER,
    done         INTEGER NOT NULL DEFAULT 0,
    created_ts   INTEGER NOT NULL,
    done_ts      INTEGER
);
";

pub fn init(db_path: &Path) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

pub fn insert(
    conn: &Connection,
    app_name: &str,
    event_type: &str,
    detail: Option<&str>,
    ts: i64,
) -> Result<i64, rusqlite::Error> {
    conn.execute(
        "INSERT INTO events (app_name, event_type, detail, ts) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![app_name, event_type, detail, ts],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn count_all(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
}

pub fn delete_by_app_and_type(
    conn: &Connection,
    app_name: &str,
    event_type: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "DELETE FROM events WHERE app_name = ?1 AND event_type = ?2",
        rusqlite::params![app_name, event_type],
    )
}

#[derive(serde::Serialize)]
pub struct DayCount {
    pub date: String,
    pub count: i64,
}

#[derive(serde::Serialize)]
pub struct AppCount {
    pub app: String,
    pub count: i64,
}

#[derive(serde::Serialize)]
pub struct Stats {
    pub today: i64,
    pub total: i64,
    pub streak: i64,
    pub heatmap: Vec<DayCount>,
    pub top_apps: Vec<AppCount>,
}

pub fn get_stats(conn: &Connection) -> Result<Stats, rusqlite::Error> {
    let today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM events WHERE date(ts, 'unixepoch', 'localtime') = date('now', 'localtime')",
        [],
        |row| row.get(0),
    )?;
    let total: i64 = count_all(conn)?;

    let active_days: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT DISTINCT date(ts, 'unixepoch', 'localtime') d FROM events WHERE d >= date('now', 'localtime', '-370 days') ORDER BY d DESC")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<String>, _>>()?
    };
    let today_str: String = conn.query_row("SELECT date('now', 'localtime')", [], |row| {
        row.get(0)
    })?;
    let yesterday_str: String = conn.query_row(
        "SELECT date('now', 'localtime', '-1 day')",
        [],
        |row| row.get(0),
    )?;

    let mut streak: i64 = 0;
    if let Some(first) = active_days.first() {
        if first == &today_str || first == &yesterday_str {
            use std::collections::HashSet;
            let set: HashSet<&String> = active_days.iter().collect();
            let mut cursor = if first == &today_str {
                today_str.clone()
            } else {
                yesterday_str.clone()
            };
            while set.contains(&cursor) {
                streak += 1;
                let prev = chrono_next_minus(&cursor);
                cursor = prev;
            }
        }
    }

    let mut heatmap_stmt = conn.prepare(
        "SELECT date(ts, 'unixepoch', 'localtime') d, COUNT(*) c FROM events WHERE d >= date('now', 'localtime', '-83 days') GROUP BY d",
    )?;
    let heatmap = heatmap_stmt
        .query_map([], |row| {
            Ok(DayCount {
                date: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<DayCount>, _>>()?;

    let mut apps_stmt = conn.prepare(
        "SELECT app_name, COUNT(*) c FROM events WHERE ts >= CAST(strftime('%s','now','-7 days') AS INTEGER) GROUP BY app_name ORDER BY c DESC LIMIT 5",
    )?;
    let top_apps = apps_stmt
        .query_map([], |row| {
            Ok(AppCount {
                app: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<AppCount>, _>>()?;

    Ok(Stats {
        today,
        total,
        streak,
        heatmap,
        top_apps,
    })
}

pub fn recent_apps(conn: &Connection, limit: i64) -> Result<Vec<AppCount>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT app_name, COUNT(*) c FROM events WHERE ts >= CAST(strftime('%s','now','-7 days') AS INTEGER) GROUP BY app_name ORDER BY c DESC LIMIT ?1",
    )?;
    let apps = stmt
        .query_map(rusqlite::params![limit], |row| {
            Ok(AppCount {
                app: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<AppCount>, _>>()?;
    Ok(apps)
}

#[derive(serde::Serialize, Clone)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub notes: Option<String>,
    pub duration_min: Option<i64>,
    pub done: bool,
    pub created_ts: i64,
}

pub fn add_task(
    conn: &Connection,
    title: &str,
    notes: Option<&str>,
    duration_min: Option<i64>,
    ts: i64,
) -> Result<Task, rusqlite::Error> {
    conn.execute(
        "INSERT INTO tasks (title, notes, duration_min, created_ts) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![title, notes, duration_min, ts],
    )?;
    Ok(Task {
        id: conn.last_insert_rowid(),
        title: title.to_string(),
        notes: notes.map(|s| s.to_string()),
        duration_min,
        done: false,
        created_ts: ts,
    })
}

fn row_to_task(row: &rusqlite::Row) -> Result<Task, rusqlite::Error> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        duration_min: row.get(3)?,
        done: row.get::<_, i64>(4)? != 0,
        created_ts: row.get(5)?,
    })
}

pub fn list_tasks(conn: &Connection) -> Result<Vec<Task>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, title, notes, duration_min, done, created_ts FROM tasks ORDER BY done ASC, (CASE WHEN done = 1 THEN done_ts ELSE created_ts END) DESC LIMIT 60",
    )?;
    let tasks = stmt
        .query_map([], |row| row_to_task(row))?
        .collect::<Result<Vec<Task>, _>>()?;
    Ok(tasks)
}

pub fn set_task_done(conn: &Connection, id: i64, done: bool, ts: i64) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "UPDATE tasks SET done = ?2, done_ts = ?3 WHERE id = ?1",
        rusqlite::params![id, done as i64, if done { Some(ts) } else { None }],
    )
}

pub fn delete_task(conn: &Connection, id: i64) -> Result<usize, rusqlite::Error> {
    conn.execute("DELETE FROM tasks WHERE id = ?1", rusqlite::params![id])
}

fn chrono_next_minus(date: &str) -> String {
    // date is YYYY-MM-DD; subtract one day via string math through chrono-free parse.
    use std::time::SystemTime;
    let _ = SystemTime::now();
    let parts: Vec<i64> = date.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return String::new();
    }
    let (mut y, mut m, mut d) = (parts[0], parts[1], parts[2]);
    d -= 1;
    if d == 0 {
        m -= 1;
        if m == 0 {
            m = 12;
            y -= 1;
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        d = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        };
    }
    format!("{y:04}-{m:02}-{d:02}")
}
