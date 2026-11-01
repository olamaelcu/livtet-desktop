# 49. System tray for window visibility and navigation

Date: 2026-10-31

## Status

Accepted

## Context

Closing the main window ended the process, so there was no way to keep a
long-running import, sync server, or reader-adjacent work alive while the
window was out of the way. The app also has four top-level destinations
(Library, Catalogs, Plugins, Settings) that were only reachable from inside the
window.

macOS and Linux both expect a resident application to expose a system tray.
Tauri supports one behind the `tray-icon` feature. Linux indicators only become
visible when a menu is attached, and the icon is a file written to
`$XDG_RUNTIME_DIR`, which the Flatpak sandbox cannot see without permission.
macOS menu bar images should be templates so the OS can tint them for light and
dark menu bars.

## Decision

Create a tray icon on macOS and Linux in `crates/livtet-desktop/src/tray.rs`
with a menu that:

- toggles the main window (`Show Window`, a check item reflecting visibility);
- opens Library, Catalogs, Plugins, or Settings;
- quits the app.

Closing the main window now hides it instead of exiting; the tray's `Quit`
item (or the macOS app menu) is the exit path. This keeps the process, its
sidecars, and its background work alive, matching platform expectations for a
tray application. Reader windows still close normally.

Navigation crosses the process boundary as the `app://navigate` event carrying
an absolute route. The web root layout subscribes once
(`web/lib/navigation.ts`) and calls `goto`; the menu is never an IPC command, so
no Specta bindings change. The route list mirrors
`web/lib/layout/navItems.ts`. The application menu shares this contract; see
[ADR 0050](0050-native-application-menu-sharing-tray-navigation.md).

Icons are assets, not generated at runtime: `icons/tray-icon.png` is
brand-green with a white glyph for Linux panels, which draw it untouched, while
macOS reuses `icons/32x32.png` with `icon_as_template(true)`. The Linux
`.deb`/`.rpm` dependency on `libayatana-appindicator3-1` is added by Tauri's
bundler; the Flatpak manifest gains
`--talk-name=org.kde.StatusNotifierWatcher` and
`--filesystem=xdg-run/tray-icon:create` so the host can render the indicator.

The tray is not created on Windows: the ask covers macOS and Linux, and Windows
keeps its existing close-to-exit behavior.

## Consequences

- Easier: background work survives a closed window; every destination is one
  tray click away; window visibility has one source of truth, read back from the
  window rather than cached in the menu.
- Harder: the app now depends on a platform indicator service. On a minimal
  Linux session without `libayatana-appindicator3`, the tray cannot appear, so
  the hidden window would leave no visible control; the quit action then lives
  only in the OS's process controls. The tray module is coverage-excluded
  (like `commands/atproto.rs`) because it needs a live desktop session.
- Risk: the check item's state is toggled by the platform before the menu event
  arrives; the handler therefore derives the target from `is_visible()` and
  rewrites the check state afterwards. Any future show/hide path must call
  `tray::sync_visibility` or the toggle goes stale.
- The event name and route set are a small contract between Rust and the web
  layer; adding a destination requires both `tray.rs` and
  `web/lib/navigation.ts` to agree.
