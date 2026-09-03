//! The notch presentation state machine.
//!
//! A single polling thread (50 ms) is the one authority for the main
//! window's stage (`Hidden | Compact | Expanded`), driven by the cursor
//! position with dwell/grace debounces — the model used by real notch
//! apps. It also toggles click-through so the transparent canvas never
//! swallows desktop clicks, and raises/sinks the window on transitions.
//!
//! Everything else (tray, commands) mutates [`Shared`] and calls
//! [`apply_stage`]; the daemon never guesses, it only observes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::windows::{self, PILL_SIZE};

/// Hover dwell before the pill expands.
const OPEN_DWELL: Duration = Duration::from_millis(180);
/// Grace period after the cursor leaves before the panel collapses.
const CLOSE_GRACE: Duration = Duration::from_millis(400);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Hidden,
    Compact,
    Expanded,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Hidden => "hidden",
            Stage::Compact => "compact",
            Stage::Expanded => "expanded",
        }
    }
}

/// State shared between the daemon, Tauri commands and the tray menu.
#[derive(Debug)]
pub struct Shared {
    pub stage: Stage,
    /// Set while transient UI (focus popover, tasks page) keeps the
    /// island expanded.
    pub pin: bool,
    /// One-shot request to collapse immediately (close button, Esc, tray).
    pub collapse_requested: bool,
    /// Live expanded-island size in logical pixels. Published by the
    /// frontend whenever the active page changes.
    pub island_w: f64,
    pub island_h: f64,
}

impl Shared {
    fn pinned(&self) -> bool {
        self.pin
    }
}

pub type SharedHandle = Arc<Mutex<Shared>>;

struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

fn contains(r: &Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

fn pill_rect(win: &tauri::WebviewWindow) -> Option<Rect> {
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    let scale = win.primary_monitor().ok().flatten()?.scale_factor();
    let w = (PILL_SIZE.0 * scale).round() as i32;
    let h = (PILL_SIZE.1 * scale).round() as i32;
    Some(Rect {
        x: pos.x + ((size.width as i32 - w) / 2),
        y: pos.y,
        w,
        h,
    })
}

fn canvas_rect(win: &tauri::WebviewWindow) -> Option<Rect> {
    let pos = win.outer_position().ok()?;
    let size = win.outer_size().ok()?;
    Some(Rect {
        x: pos.x,
        y: pos.y,
        w: size.width as i32,
        h: size.height as i32,
    })
}

/// The expanded island's physical rect: horizontally centered in the
/// canvas, using the live size published by the frontend.
fn expanded_island_rect(win: &tauri::WebviewWindow, w: f64, h: f64) -> Option<Rect> {
    let canvas = canvas_rect(win)?;
    let scale = win.primary_monitor().ok().flatten()?.scale_factor();
    let w = (w * scale).round() as i32;
    let h = (h * scale).round() as i32;
    Some(Rect {
        x: canvas.x + ((canvas.w - w) / 2),
        y: canvas.y,
        w,
        h,
    })
}

/// Applies a stage transition: notifies the webview and re-positions the
/// window in the z-order (raised while expanded, sunk otherwise).
pub fn apply_stage(app: &AppHandle, stage: Stage) {
    let _ = app.emit_to(
        "main",
        "notch://stage",
        json!({ "stage": stage.as_str() }),
    );
    #[cfg(windows)]
    if let Some(win) = app.get_webview_window("main") {
        if let Ok(hwnd) = win.hwnd() {
            windows::z_order::set_z_order(hwnd.0 as isize, stage != Stage::Expanded);
        }
    }
}

/// Immediately expands the island (pill click affordance).
pub fn expand_now(shared: &SharedHandle, app: &AppHandle) {
    let mut s = match shared.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    if s.stage == Stage::Hidden {
        return;
    }
    s.stage = Stage::Expanded;
    drop(s);
    apply_stage(app, Stage::Expanded);
}

/// Immediately collapses the island (close button, Esc).
pub fn collapse_now(shared: &SharedHandle, app: &AppHandle) {
    {
        let mut s = match shared.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        if s.stage == Stage::Hidden {
            return;
        }
        s.stage = Stage::Compact;
        // The cursor is usually still over the canvas here; block the
        // hover-dwell until it leaves the pill, or the panel would
        // instantly re-expand and the close button would look broken.
        s.collapse_requested = true;
    }
    apply_stage(app, Stage::Compact);
}

pub fn spawn(app: AppHandle, shared: SharedHandle) {
    #[cfg(windows)]
    spawn_windows(app, shared);
    #[cfg(not(windows))]
    let _ = (app, shared);
}

#[cfg(windows)]
fn spawn_windows(app: AppHandle, shared: SharedHandle) {
    const VK_LBUTTON: i32 = 0x01;

    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }

    std::thread::spawn(move || {
        let mut dwell_since: Option<Instant> = None;
        let mut outside_since: Option<Instant> = None;
        let mut block_dwell = false;
        let mut ignore = true;
        let mut prev_left_down = false;

        loop {
            std::thread::sleep(POLL_INTERVAL);

            let (stage, pinned, collapse_requested, island_w, island_h) = {
                let s = match shared.lock() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                (s.stage, s.pinned(), s.collapse_requested, s.island_w, s.island_h)
            };
            if stage == Stage::Hidden {
                dwell_since = None;
                outside_since = None;
                continue;
            }

            let Some(win) = app.get_webview_window("main") else {
                continue;
            };
            let Some(pill) = pill_rect(&win) else {
                continue;
            };
            let Some(island) = expanded_island_rect(&win, island_w, island_h) else {
                continue;
            };
            let Ok(cursor) = app.cursor_position() else {
                continue;
            };
            let (cx, cy) = (cursor.x.round() as i32, cursor.y.round() as i32);

            let over_island = contains(&island, cx, cy);
            let over_pill = contains(&pill, cx, cy);

            // Global left-click edge: pressing outside the expanded island
            // dismisses it (same contract as the pill), routed through the
            // frontend when transient UI is pinned.
            let left_down = (unsafe { GetAsyncKeyState(VK_LBUTTON) } as u16) & 0x8000 != 0;
            let pressed_outside = left_down && !prev_left_down && stage == Stage::Expanded && !over_island;
            prev_left_down = left_down;
            if pressed_outside {
                if pinned {
                    let _ = app.emit_to("main", "notch://outside", ());
                } else {
                    if let Ok(mut s) = shared.lock() {
                        s.stage = Stage::Compact;
                        s.collapse_requested = true;
                    }
                    apply_stage(&app, Stage::Compact);
                    dwell_since = None;
                    outside_since = None;
                    block_dwell = true;
                    continue;
                }
            }

            // Consume one-shot collapse requests: re-arm the hover dwell
            // only after the pointer has left the pill.
            let mut block = block_dwell || collapse_requested;
            if block && !over_pill {
                block = false;
            }

            let mut next = stage;
            match stage {
                Stage::Compact => {
                    if over_pill && !block {
                        let since = *dwell_since.get_or_insert(Instant::now());
                        if since.elapsed() >= OPEN_DWELL {
                            next = Stage::Expanded;
                        }
                    } else {
                        dwell_since = None;
                    }
                }
                Stage::Expanded => {
                    if over_island {
                        outside_since = None;
                    } else if !pinned {
                        let since = *outside_since.get_or_insert(Instant::now());
                        if since.elapsed() >= CLOSE_GRACE {
                            next = Stage::Compact;
                        }
                    }
                }
                Stage::Hidden => {}
            }

            if collapse_requested || next != stage {
                if let Ok(mut s) = shared.lock() {
                    s.collapse_requested = false;
                    s.stage = next;
                }
                if next != stage {
                    apply_stage(&app, next);
                }
                dwell_since = None;
                outside_since = None;
            }
            block_dwell = block && next == Stage::Compact;

            // Click-through: only the visible island receives the pointer.
            let want_ignore = if next == Stage::Expanded {
                !over_island
            } else {
                !over_pill
            };
            if want_ignore != ignore
                && win.set_ignore_cursor_events(want_ignore).is_ok()
            {
                ignore = want_ignore;
            }
        }
    });
}
