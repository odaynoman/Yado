//! Per-app icon cache.
//!
//! Extracts the shell icon of a running app's exe (the moment the focus
//! tracker sees it), stores it as a PNG next to the database, and serves
//! it to the UI as a data URL. This powers the settings chips, the task
//! rows and the media card with real application icons.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::Engine as _;

static ICON_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init(data_dir: &Path) {
    let dir = data_dir.join("icons");
    let _ = std::fs::create_dir_all(&dir);
    let _ = ICON_DIR.set(dir);
}

fn icon_path(exe_name: &str) -> Option<PathBuf> {
    Some(ICON_DIR.get()?.join(format!("{}.png", exe_name.to_lowercase())))
}

/// Fire-and-forget extraction; safe to call on every focus change.
pub fn ensure_cached(exe_name: &str, exe_path: &Path) {
    let Some(target) = icon_path(exe_name) else {
        return;
    };
    if target.exists() {
        return;
    }
    let exe_path = exe_path.to_path_buf();
    std::thread::spawn(move || {
        if let Err(e) = extract_to_png(&exe_path, &target) {
            eprintln!("[icons] extract {} failed: {e}", exe_path.display());
        }
    });
}

/// Returns the cached icon as a `data:image/png;base64,…` URL.
pub fn data_url(exe_name: &str) -> Option<String> {
    let bytes = std::fs::read(icon_path(exe_name)?).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(windows)]
fn extract_to_png(exe_path: &Path, target: &Path) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO, LR_DEFAULTSIZE,
    };

    const SIZE: i32 = 48;

    // The generated binding takes a fixed 260-wchar path buffer.
    let mut wide = [0u16; 260];
    let path_str = exe_path.to_string_lossy();
    for (i, c) in path_str.encode_utf16().take(259).enumerate() {
        wide[i] = c;
    }

    unsafe {
        let mut hicon = HICON::default();
        let extracted = PrivateExtractIconsW(
            &wide,
            0,
            SIZE,
            SIZE,
            Some(std::slice::from_mut(&mut hicon)),
            None,
            LR_DEFAULTSIZE.0,
        );
        if extracted == 0 || hicon.is_invalid() {
            return Err("no icon resource".into());
        }

        let result = (|| -> Result<(), String> {
            let mut info = ICONINFO::default();
            GetIconInfo(hicon, &mut info).map_err(|e| format!("GetIconInfo: {e}"))?;

            let cleanup = |info: &ICONINFO| {
                if !info.hbmMask.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(info.hbmMask.0));
                }
                if !info.hbmColor.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ(info.hbmColor.0));
                }
                let _ = DestroyIcon(hicon);
            };

            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: SIZE,
                    biHeight: -SIZE, // top-down
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0, // BI_RGB
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                ..Default::default()
            };

            let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
            let dc = CreateCompatibleDC(None);
            let ok = GetDIBits(
                dc,
                info.hbmColor,
                0,
                SIZE as u32,
                Some(pixels.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            if ok == 0 {
                cleanup(&info);
                return Err("GetDIBits failed".into());
            }
            cleanup(&info);

            // BGRA -> RGBA.
            for px in pixels.chunks_exact_mut(4) {
                px.swap(0, 2);
            }

            let img: image::RgbaImage =
                image::ImageBuffer::from_raw(SIZE as u32, SIZE as u32, pixels)
                    .ok_or("bad pixel buffer")?;
            img.save(target).map_err(|e| e.to_string())
        })();

        result
    }
}

#[cfg(not(windows))]
fn extract_to_png(_exe_path: &Path, _target: &Path) -> Result<(), String> {
    Err("unsupported platform".into())
}
