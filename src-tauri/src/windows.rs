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

    /// Whether a *fullscreen app* covers the notch point right now.
    ///
    /// Two authoritative signals, OR-ed: the app the user is working in
    /// (foreground window), or any fullscreen window above us in the
    /// z-order (a maximized app behind a small floating window). A
    /// candidate counts only if it is a real, painted, fullscreen app —
    /// invisible fullscreen overlays, palettes, cloaked shell windows
    /// and the wallpaper itself never count.
    pub fn point_covered_by_other(x: i32, y: i32, our: isize) -> bool {
        #[repr(C)]
        struct Rect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }

        const GWL_EXSTYLE: i32 = -20;
        const WS_EX_TRANSPARENT: i32 = 0x20;
        const WS_EX_TOOLWINDOW: isize = 0x80;
        const GW_HWNDNEXT: u32 = 2;
        const SM_CXSCREEN: i32 = 0;
        const SM_CYSCREEN: i32 = 1;
        const DWMWA_CLOAKED: u32 = 14;

        #[link(name = "user32")]
        extern "system" {
            fn GetForegroundWindow() -> isize;
            fn GetTopWindow(h: isize) -> isize;
            fn GetWindow(h: isize, cmd: u32) -> isize;
            fn IsWindowVisible(h: isize) -> i32;
            fn IsIconic(h: isize) -> i32;
            fn GetWindowLongW(h: isize, index: i32) -> i32;
            fn GetWindowRect(h: isize, rect: *mut Rect) -> i32;
            fn GetSystemMetrics(index: i32) -> i32;
            fn GetClassNameW(h: isize, buf: *mut u16, max: i32) -> i32;
        }

        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmGetWindowAttribute(h: isize, attr: u32, val: *mut u32, size: u32) -> i32;
        }

        // The desktop itself is never "an app covering the notch".
        unsafe fn is_shell_wallpaper(h: isize) -> bool {
            let mut buf = [0u16; 16];
            let n = GetClassNameW(h, buf.as_mut_ptr(), buf.len() as i32);
            if n <= 0 {
                return false;
            }
            let name = String::from_utf16_lossy(&buf[..n as usize]);
            name == "Progman" || name == "WorkerW"
        }

        // Cloaked windows paint nothing (compositor ground truth):
        // Widgets board when closed, suspended UWP frames, etc.
        unsafe fn is_cloaked(h: isize) -> bool {
            let mut v: u32 = 0;
            let ok = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut v, 4);
            ok == 0 && v != 0
        }

        // A real, interactable app window — not an overlay, palette,
        // ghost or the wallpaper.
        unsafe fn is_app_window(h: isize) -> bool {
            IsWindowVisible(h) != 0
                && IsIconic(h) == 0
                && (GetWindowLongW(h, GWL_EXSTYLE) & WS_EX_TRANSPARENT) == 0
                && (GetWindowLongW(h, GWL_EXSTYLE) & WS_EX_TOOLWINDOW as i32) == 0
                && !is_shell_wallpaper(h)
                && !is_cloaked(h)
        }

        unsafe fn window_rect(h: isize) -> Option<Rect> {
            let mut r = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            (GetWindowRect(h, &mut r) != 0).then_some(r)
        }

        unsafe {
            // The notch lives top-center of the primary monitor, so a
            // covering *fullscreen* app is one whose rect contains the
            // whole screen. (Maximized windows overshoot by the border
            // size, which containment absorbs naturally.)
            let sw = GetSystemMetrics(SM_CXSCREEN);
            let sh = GetSystemMetrics(SM_CYSCREEN);
            let covers = |h: isize| {
                is_app_window(h)
                    && window_rect(h).is_some_and(|r| {
                        x >= r.left
                            && x < r.right
                            && y >= r.top
                            && y < r.bottom
                            && r.left <= 0
                            && r.top <= 0
                            && r.right >= sw
                            && r.bottom >= sh
                    })
            };

            let fg = GetForegroundWindow();
            if fg != 0 && fg != our && covers(fg) {
                return true;
            }
            let mut cur = GetTopWindow(0);
            while cur != 0 {
                if cur == our {
                    return false;
                }
                if covers(cur) {
                    return true;
                }
                cur = GetWindow(cur, GW_HWNDNEXT);
            }
            false
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
