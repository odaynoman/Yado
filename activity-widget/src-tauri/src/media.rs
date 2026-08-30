//! Windows system media integration (SMTC).
//!
//! Polls the GlobalSystemMediaTransportControlsSessionManager — the same
//! pipeline the Windows volume flyout uses — so every app that exposes
//! system media controls (Spotify, Chrome, players, …) shows up in the
//! widget with title/artist/app, transport controls and a seekable
//! timeline.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows_collections::IVectorView;

const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// The session the widget is currently bound to; commands act on it.
#[derive(Default)]
pub struct MediaHandle(pub Mutex<Option<GlobalSystemMediaTransportControlsSession>>);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
    pub position_sec: f64,
    pub duration_sec: f64,
}

fn hstring_str(h: &windows::core::HSTRING) -> String {
    h.to_string_lossy()
}

fn seconds(t: windows::Foundation::TimeSpan) -> f64 {
    t.Duration as f64 / 10_000_000.0
}

/// Blocks on a WinRT async operation (called from the poll thread only).
fn await_op<T: windows::core::RuntimeType>(
    op: windows::core::Result<windows_future::IAsyncOperation<T>>,
) -> Option<T> {
    op.ok()?.get().ok()
}

/// "Microsoft.ZuneMusic_8wekyb3d8bbwe!App" -> "ZuneMusic"; "chrome" -> "Chrome".
fn pretty_app(aumid: &str) -> String {
    let base = aumid.split('!').next().unwrap_or(aumid);
    let name = base.split('_').next().unwrap_or(base);
    let leaf = name.rsplit(['.', '\\', '/']).next().unwrap_or(name);
    let mut chars = leaf.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Picks the active session: the playing one if any, else the first.
fn pick_session(
    sessions: &IVectorView<GlobalSystemMediaTransportControlsSession>,
) -> Option<GlobalSystemMediaTransportControlsSession> {
    let mut fallback = None;
    for session in sessions {
        let playing = session
            .GetPlaybackInfo()
            .ok()
            .and_then(|info| info.PlaybackStatus().ok())
            .is_some_and(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
        if playing {
            return Some(session);
        }
        if fallback.is_none() {
            fallback = Some(session);
        }
    }
    fallback
}

fn read_info(
    session: &GlobalSystemMediaTransportControlsSession,
) -> Option<MediaInfo> {
    let playing = session
        .GetPlaybackInfo()
        .ok()?
        .PlaybackStatus()
        .ok()?
        == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;

    let props = await_op(session.TryGetMediaPropertiesAsync())?;
    let app = pretty_app(&hstring_str(&session.SourceAppUserModelId().ok()?));

    let (position_sec, duration_sec) = match session.GetTimelineProperties() {
        Ok(t) => (
            t.Position().map(seconds).unwrap_or(0.0),
            t.EndTime().map(seconds).unwrap_or(0.0),
        ),
        Err(_) => (0.0, 0.0),
    };

    Some(MediaInfo {
        title: hstring_str(&props.Title().ok()?),
        artist: hstring_str(&props.Artist().ok()?),
        app,
        playing,
        position_sec,
        duration_sec,
    })
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let manager = match await_op(GlobalSystemMediaTransportControlsSessionManager::RequestAsync())
        {
            Some(m) => m,
            None => {
                eprintln!("[media] SMTC unavailable");
                return;
            }
        };

        let mut cached: Option<MediaInfo> = None;
        loop {
            std::thread::sleep(POLL_INTERVAL);

            let Ok(sessions) = manager.GetSessions() else {
                continue;
            };
            let Some(session) = pick_session(&sessions) else {
                if cached.is_some() {
                    cached = None;
                    let _ = app.emit("media://state", Option::<MediaInfo>::None);
                    if let Some(handle) = app.try_state::<MediaHandle>() {
                        *handle.0.lock().unwrap() = None;
                    }
                }
                continue;
            };

            let Some(info) = read_info(&session) else {
                continue;
            };
            // Round the position so the 1 s poll doesn't re-emit constantly.
            let mut emit_info = info.clone();
            emit_info.position_sec = (emit_info.position_sec * 10.0).round() / 10.0;

            if cached.as_ref() != Some(&emit_info) {
                cached = Some(emit_info);
                if let Some(handle) = app.try_state::<MediaHandle>() {
                    *handle.0.lock().unwrap() = Some(session.clone());
                }
                let _ = app.emit("media://state", Some(info));
            }
        }
    });
}

fn with_session<R>(
    state: &State<MediaHandle>,
    f: impl FnOnce(&GlobalSystemMediaTransportControlsSession) -> Option<R>,
) -> Option<R> {
    let guard = state.0.lock().ok();
    let session = guard.as_ref()?.as_ref()?;
    f(session)
}

macro_rules! transport_cmd {
    ($name:ident, $op:path) => {
        #[tauri::command]
        pub fn $name(state: State<'_, MediaHandle>) -> Result<(), String> {
            with_session(&state, |s| await_op($op(s)).map(|_| ()))
                .ok_or_else(|| "no active media session".into())
        }
    };
}

transport_cmd!(media_play, GlobalSystemMediaTransportControlsSession::TryPlayAsync);
transport_cmd!(media_pause, GlobalSystemMediaTransportControlsSession::TryPauseAsync);
transport_cmd!(media_next, GlobalSystemMediaTransportControlsSession::TrySkipNextAsync);
transport_cmd!(media_prev, GlobalSystemMediaTransportControlsSession::TrySkipPreviousAsync);

#[tauri::command]
pub fn media_seek(state: State<'_, MediaHandle>, position_sec: f64) -> Result<(), String> {
    let ticks = (position_sec * 10_000_000.0).round() as i64;
    with_session(&state, |s| {
        await_op(s.TryChangePlaybackPositionAsync(ticks)).map(|_| ())
    })
    .ok_or_else(|| "no active media session".into())
}
