# Yado Notch

A native-feeling **activity notch** for Windows — a compact pill that lives at the
top-center of your screen, expands into a dashboard on hover, and quietly tracks
everything you do: apps you focus, files you save, focus sessions you run, and
music you play.

Built with **Tauri 2 + TypeScript**. No framework runtime, no telemetry, everything local.

## Features

**Tracking** — apps you focus and file saves are logged to a local SQLite database and
rendered as a month heatmap, weekly usage bars and per-app statistics.

**Tasks** — calendar scheduling, due dates, duration estimates, notes, a done list and
quick rescheduling. A single-source store keeps the dashboard preview and the tasks
page perfectly in sync.

**Focus sessions** — pick any duration (down to the second), get a countdown in the
pill, a progress line on the dock, and a system chime + native toast when time is up.

**Media** — play/pause/prev/next/seek for whatever app is currently playing
(Spotify, Chrome, players…) via the Windows System Media Transport Controls.

**Notch behavior** — hover to expand, click-outside to dismiss, spring motion
everywhere, and a desktop-level widget that sinks under your windows instead of
fighting them for attention.

**App icons** — real per-app icons are extracted from executables and shown in
usage stats, tracking switches and the media strip.

**Single instance** — launching the app twice just refocuses the running notch.

## Install

Download `Yado Notch_x.x.x_x64-setup.exe` from
[Releases](../../releases) and run it. Launch **Yado Notch** from the Start Menu.

## Build from source

```bash
npm install
npm run tauri dev     # develop
npm run tauri build   # installer + portable exe
```

Requirements: Node.js 18+, Rust (MSVC toolchain), WebView2 (preinstalled on Windows 11).

## How it works

The window is a *fixed transparent canvas*. A single Rust "notch daemon" polls the
cursor at 50 ms and owns the presentation state machine (`compact ⇄ expanded`): hover
dwell, close grace, click-through outside the visible island, and z-order transitions
(raise on expand, sink to the desktop layer otherwise). The visible island morphs
inside the canvas with CSS springs — the window itself is never resized at runtime.

Key sources:

```
src-tauri/src/notch_daemon.rs   stage state machine, click-through, z-order
src-tauri/src/tracker.rs        app-focus + file-save tracking, allowlist
src-tauri/src/media.rs          SMTC media integration
src-tauri/src/db.rs             SQLite schema + queries
src/tasksStore.ts               single-source task state (frontend)
```

## Configuration

Tracking is configurable via `%APPDATA%\com.yado.notch\config.json`
(hot-reloads in ~1.5 s):

```json
{
  "mode": "all",
  "apps": ["chrome.exe", "cursor.exe"],
  "watch_folders": ["D:\\projects\\app"]
}
```

- `mode`: `"all"` tracks every app, `"allowlist"` only the chosen ones
- `watch_folders`: folders watched for save events (omit to auto-watch
  Desktop, Documents, Downloads, Pictures and Videos)

## License

[MIT](LICENSE)
