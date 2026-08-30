//! System sound effects played through the Windows audio session.

use std::time::Duration;

/// Double-chime that announces a finished focus session. Runs on its own
/// thread so the caller (a Tauri command) never blocks the UI.
pub fn notification_chime() {
    std::thread::spawn(|| {
        #[cfg(windows)]
        {
            #[link(name = "winmm")]
            extern "system" {
                fn PlaySoundW(pszsound: *const u16, hmod: isize, fdwsound: u32) -> i32;
            }

            const SND_ASYNC: u32 = 0x0001;
            const SND_NODEFAULT: u32 = 0x0002;
            const SND_FILENAME: u32 = 0x0002_0000;
            const SND_ALIAS: u32 = 0x0000;

            let play = |name: &str, flags: u32| -> bool {
                let wide: Vec<u16> = name
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe { PlaySoundW(wide.as_ptr(), 0, flags) != 0 }
            };

            let file = "C:\\Windows\\Media\\Windows Notify.wav";
            let file_flags = SND_ASYNC | SND_NODEFAULT | SND_FILENAME;
            let first = play(file, file_flags);
            eprintln!("[sound] chime 1: {first}");
            if first {
                // Second chime after the first ring finishes: unmistakable.
                std::thread::sleep(Duration::from_millis(1100));
                let second = play(file, file_flags);
                eprintln!("[sound] chime 2: {second}");
            } else {
                let fallback = play("SystemDefault", SND_ASYNC | SND_ALIAS);
                eprintln!("[sound] fallback chime: {fallback}");
            }
        }
    });
}
