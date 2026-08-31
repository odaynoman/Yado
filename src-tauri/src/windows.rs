//! Window geometry and z-order primitives.
//!
//! The main window is a *fixed canvas*: it is never resized at runtime.
//! The visible island (pill -> dashboard -> tasks) animates inside it,
//! while [`crate::notch_daemon`] owns stage transitions, click-through
//! and z-order. Placement helpers here only run once at startup.

use tauri::{LogicalSize, PhysicalPosition, WebviewWindow};

/// Main window: a static transparent canvas covering the maximum
/// expansion area of the island (the tasks page is the largest stage).
pub const CANVAS_SIZE: (f64, f64) = (600.0, 464.0);
/// Compact island (the notch pill) rendered inside the canvas.
pub const PILL_SIZE: (f64, f64) = (340.0, 44.0);
/// Dashboard island (default expanded stage).
pub const ISLAND_DASHBOARD_SIZE: (f64, f64) = (474.0, 312.0);

#[cfg(windows)]
pub mod z_order {
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const SW_HIDE: i32 = 0;
    const SW_SHOWNA: i32 = 8;
    const SWP_FLAGS: u32 = 0x0001 | 0x0002 | 0x0010 | 0x0200; // NOSIZE|NOMOVE|NOACTIVATE|NOOWNERZORDER
    const HWND_TOP: isize = 0;
    const HWND_BOTTOM: isize = 1;

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

    /// Makes the window skip the taskbar and lose its taskbar button.
    /// Tauri's `skipTaskbar` alone is unreliable for transparent windows,
    /// so the extended styles are enforced directly.
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

    /// Raised when the island is expanded so the panel is usable; sunk to
    /// the desktop layer otherwise (classic desktop-widget behavior).
    pub fn set_z_order(hwnd: isize, to_bottom: bool) {
        unsafe {
            SetWindowPos(
                hwnd,
                if to_bottom { HWND_BOTTOM } else { HWND_TOP },
                0,
                0,
                0,
                0,
                SWP_FLAGS,
            );
        }
    }
}

/// Centers the canvas horizontally at the very top of the primary monitor
/// (the "notch" position) and pins its logical size.
pub fn place_main(win: &WebviewWindow) {
    let Ok(Some(monitor)) = win.primary_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let width = (CANVAS_SIZE.0 * scale).round() as i32;
    let x = ((monitor.size().width as f64 - width as f64) / 2.0).round() as i32;
    let _ = win.set_size(LogicalSize::new(CANVAS_SIZE.0, CANVAS_SIZE.1));
    let _ = win.set_position(PhysicalPosition::new(x, 0));
}
