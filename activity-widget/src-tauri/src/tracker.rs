use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OWN_PROCESS: &str = "activity-widget";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AppFilterMode {
    All,
    Allowlist,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrackerConfig {
    #[serde(default = "default_mode")]
    pub mode: AppFilterMode,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub watch_folders: Option<Vec<String>>,
}

fn default_mode() -> AppFilterMode {
    AppFilterMode::All
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            mode: AppFilterMode::All,
            apps: Vec::new(),
            watch_folders: None,
        }
    }
}

pub struct FocusState {
    pub last_app: Mutex<Option<String>>,
}

pub fn config_path(data_dir: &PathBuf) -> PathBuf {
    data_dir.join("config.json")
}

pub fn load_config(path: &PathBuf) -> TrackerConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => {
            let json = serde_json::to_string_pretty(&TrackerConfig::default())
                .unwrap_or_else(|_| "{}".into());
            let _ = std::fs::write(path, json);
            TrackerConfig::default()
        }
    }
}

pub fn should_track(config: &TrackerConfig, exe_name: &str) -> bool {
    match config.mode {
        AppFilterMode::All => true,
        AppFilterMode::Allowlist => config
            .apps
            .iter()
            .any(|a| a.eq_ignore_ascii_case(exe_name)),
    }
}

#[cfg(windows)]
pub mod foreground {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> isize;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn GetWindowTextW(hwnd: isize, buf: *mut u16, max: i32) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn CloseHandle(handle: isize) -> i32;
        fn QueryFullProcessImageNameW(
            proc: isize,
            flags: u32,
            buf: *mut u16,
            size: *mut u32,
        ) -> i32;
    }

    pub struct ForegroundInfo {
        pub exe_name: String,
        pub window_title: String,
    }

    pub fn get() -> Option<ForegroundInfo> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd == 0 {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle == 0 {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let exe_path = if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) != 0
            {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                String::new()
            };
            CloseHandle(handle);

            let mut tbuf = [0u16; 512];
            let tlen = GetWindowTextW(hwnd, tbuf.as_mut_ptr(), 512);
            let window_title = if tlen > 0 {
                String::from_utf16_lossy(&tbuf[..tlen as usize])
            } else {
                String::new()
            };

            let exe_name = std::path::Path::new(&exe_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if exe_name.is_empty() {
                return None;
            }
            Some(ForegroundInfo {
                exe_name,
                window_title,
            })
        }
    }
}

#[cfg(windows)]
pub fn spawn_focus_tracker(data_dir: PathBuf) {
    std::thread::spawn(move || {
        let conn = match crate::db::init(&data_dir.join("activities.db")) {
            Ok(c) => c,
            Err(e) => {
                println!("[tracker] db open failed: {e}");
                return;
            }
        };
        let state = FocusState {
            last_app: Mutex::new(None),
        };
        let mut config = load_config(&config_path(&data_dir));
        let mut config_mtime = std::fs::metadata(config_path(&data_dir))
            .and_then(|m| m.modified())
            .ok();

        loop {
            std::thread::sleep(Duration::from_millis(1500));

            if let Ok(m) = std::fs::metadata(config_path(&data_dir)) {
                if let Ok(modified) = m.modified() {
                    if config_mtime.as_ref() != Some(&modified) {
                        config = load_config(&config_path(&data_dir));
                        config_mtime = Some(modified);
                        println!("[tracker] config reloaded: mode={:?} apps={:?}", config.mode, config.apps);
                    }
                }
            }

            let Some(info) = foreground::get() else { continue };
            if info.exe_name.starts_with(OWN_PROCESS) {
                continue;
            }
            let mut last = state.last_app.lock().unwrap();
            if last.as_deref() == Some(info.exe_name.as_str()) {
                continue;
            }
            if !should_track(&config, &info.exe_name) {
                *last = Some(info.exe_name);
                continue;
            }
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let detail = if info.window_title.is_empty() {
                None
            } else {
                Some(info.window_title)
            };
            match crate::db::insert(&conn, &info.exe_name, "app_focus", detail.as_deref(), ts) {
                Ok(_) => println!("[tracker] focus: {} ({})", info.exe_name, detail.unwrap_or_default()),
                Err(e) => println!("[tracker] insert failed: {e}"),
            }
            *last = Some(info.exe_name);
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_focus_tracker(_data_dir: PathBuf) {}

fn extension_bucket(ext: &str) -> &'static str {
    match ext {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "cs" | "rb" | "php" | "swift" | "kt" | "html" | "css" | "scss" | "json" | "yaml"
        | "yml" | "toml" | "sql" | "sh" | "ps1" | "bat" | "vue" | "svelte" | "lua" | "dart" => {
            "Code"
        }
        "md" | "txt" | "doc" | "docx" | "pdf" | "xls" | "xlsx" | "ppt" | "pptx" | "csv" | "rtf"
        | "odt" | "tex" => "Docs",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "heic" => "Images",
        "mp4" | "mkv" | "mov" | "avi" | "mp3" | "wav" | "flac" | "aac" => "Media",
        _ => "Files",
    }
}

const IGNORED_EXTS: &[&str] = &[
    "tmp", "temp", "crdownload", "part", "partial", "swp", "log", "lnk", "lock",
];

pub fn spawn_file_watcher(data_dir: PathBuf) {
    std::thread::spawn(move || {
        let conn = match crate::db::init(&data_dir.join("activities.db")) {
            Ok(c) => c,
            Err(e) => {
                println!("[watcher] db open failed: {e}");
                return;
            }
        };
        let config = load_config(&config_path(&data_dir));

        let folders: Vec<PathBuf> = match &config.watch_folders {
            Some(list) if !list.is_empty() => list.iter().map(PathBuf::from).collect(),
            _ => [
                dirs::desktop_dir(),
                dirs::document_dir(),
                dirs::download_dir(),
                dirs::picture_dir(),
                dirs::video_dir(),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        if folders.is_empty() {
            println!("[watcher] no folders to watch");
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => {
                println!("[watcher] init failed: {e}");
                return;
            }
        };
        use notify::Watcher;
        for f in &folders {
            match watcher.watch(f, notify::RecursiveMode::Recursive) {
                Ok(_) => println!("[watcher] watching {}", f.display()),
                Err(e) => println!("[watcher] watch failed {}: {e}", f.display()),
            }
        }

        let mut last_seen: HashMap<PathBuf, Instant> = HashMap::new();

        for res in rx {
            let Ok(event) = res else { continue };
            let candidate =
                matches!(event.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_));
            if !candidate {
                continue;
            }
            for path in event.paths {
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if name.starts_with('.') || name.starts_with("~$") {
                    continue;
                }
                let ext = path
                    .extension()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if IGNORED_EXTS.contains(&ext.as_str()) {
                    continue;
                }
                let now = Instant::now();
                if let Some(t) = last_seen.get(&path) {
                    if now.duration_since(*t) < Duration::from_millis(1000) {
                        continue;
                    }
                }
                last_seen.insert(path.clone(), now);
                if last_seen.len() > 10_000 {
                    last_seen.clear();
                }
                let app = extension_bucket(&ext);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                match crate::db::insert(
                    &conn,
                    app,
                    "file_save",
                    Some(&path.to_string_lossy()),
                    ts,
                ) {
                    Ok(_) => println!("[watcher] save: {} ({})", path.display(), app),
                    Err(e) => println!("[watcher] insert failed: {e}"),
                }
            }
        }
    });
}
