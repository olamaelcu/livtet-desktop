# macOS Integration Improvements

## Changes Made

### 1. `crates/livtet-desktop/tauri.conf.json` — Window Configuration

Added `titleBarStyle: "overlay"` and `hiddenTitle: true` to the main window definition. These coordinate with the overlay titlebar created by `tauri-plugin-decorum`'s `create_overlay_titlebar()` call in `lib.rs::run()`. Without these settings, Tauri initializes the window with standard decorations before the plugin overrides them, which can cause a flash of native titlebar content on app launch.

### 2. `crates/livtet-desktop/src/lib.rs` — Rust-side Window Setup

Enhanced the macOS block in `run()` with two additional calls:

- **`window.set_window_level(25)?`** — NSWindowLevel 25 (`NSFloatingWindowLevel`). The reader window opens as a second Tauri window; placing it above other windows keeps it visible when users switch apps.
- **`window.set_shadow(true)?`** — Ensures the overlay titlebar window casts a shadow, giving it visual depth. Without this, transparent macOS windows float without a drop shadow, looking detached from the desktop.

## Why These Changes Matter

| Setting | Effect | Without it |
|---------|--------|------------|
| `titleBarStyle: "overlay"` | Window starts with overlay titlebar | Flash of native titlebar on launch |
| `hiddenTitle: true` | Hides title text from titlebar | Unnecessary title text in overlay bar |
| `set_window_level(25)` | Reader window floats above other apps | Reader window falls behind main app |
| `set_shadow(true)` | Drop shadow on transparent window | Window appears detached/floating |

## tauri-plugin-decorum Usage

The project uses the forked `olamaelcu/tauri-plugin-decorum` (v1.1.1) as a git dependency in `crates/livtet-desktop/Cargo.toml`. It provides three macOS helpers used here:

- `create_overlay_titlebar()` — hides native titlebar, enables custom window controls
- `set_traffic_lights_inset(x, y)` — adjusts position of macOS traffic light buttons
- `make_transparent()` — enables window transparency without private API

The plugin's README documents additional capabilities not yet adopted:

- **CSS custom titlebar buttons**: classes `.decorum-tb-btn`, `#decorum-tb-minimize`, `#decorum-tb-maximize`, `#decorum-tb-close` — available if custom-styled buttons are desired later
- **`set_window_level(level)`** — now adopted (NSWindowLevel constant)
- Deep-link scheme registration on macOS/Android/iOS is build-time only (no runtime registration needed)

## Verification

- `tauri.conf.json` passes JSON validation
- macOS-specific code is guarded by `#[cfg(target_os = "macos")]` — no impact on Linux/Windows builds
- ADR-0033's per-route layout change explicitly notes the web layer no longer reserves space for window controls, consistent with `hiddenTitle: true`
