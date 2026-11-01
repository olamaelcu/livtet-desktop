# 50. Native application menu sharing tray navigation

Date: 2026-10-31

## Status

Accepted

## Context

Tauri installed no application menu. On macOS that left users without the
conventional About, Hide, and Quit roles; on Linux the window had no menu bar
at all. Top-level destinations were reachable only from the in-app nav rail,
the command palette, and the tray (ADR 0049).

Tauri menus are built from the same `muda` model as the tray menu. Three
constraints shape how they fit this app:

- Menu events are dispatched globally. A second `on_menu_event` listener would
  receive the same event as the tray's listener and run any matching action
  twice.
- macOS supports only an app-wide menu. Linux supports per-window menus, and an
  app-wide menu is inherited by windows created later, including reader
  windows.
- Several predefined roles are unsupported on Linux: Quit, Close Window,
  Minimize, Maximize, and Fullscreen. About and the Edit roles are supported.

The web layer already owns user-rebindable navigation hotkeys
(`web/lib/hotkeys/commands.ts`), so native accelerators would create a second,
unconfigurable source for the same keys.

## Decision

Build the application menu in Rust in `crates/livtet-desktop/src/menu.rs`.

- macOS gets the conventional app submenu first (About, Services, Hide, Hide
  Others, Show All, Quit), then Edit, Navigate, and Window. All roles are
  predefined except Navigate.
- Linux gets File (a custom Quit), Edit, Navigate, and Help (About). Linux has
  no window menu because its predefined window roles are unsupported.
- Navigate carries Library, Catalogs, Plugins, and Settings. Items have no
  native accelerators; the web layer remains the only hotkey source.
- On macOS the menu is installed app-wide with `AppHandle::set_menu`. On Linux
  it is installed on the main window only with `WebviewWindow::set_menu`, so
  reader windows stay chrome-free instead of inheriting a menu bar.
- One `Builder::on_menu_event` handler (`menu::handle_event`) dispatches every
  native menu event, tray included. The tray no longer registers its own
  handler.
- Both surfaces share one navigation contract: `tray::open_route` focuses the
  main window and emits `app://navigate` to that window's webview. The web
  listener moved from `web/lib/tray.ts` to `web/lib/navigation.ts`; the event
  name changed from `tray://navigate` to `app://navigate` because the menu now
  uses it too.
- The menu is not built on Windows: `create_overlay_titlebar` removes window
  decorations there, so a native menu bar would not be visible.

## Consequences

- Easier: macOS gets the platform-standard app menu and a discoverable Quit;
  Linux gets a conventional menu bar; both reuse the tray's navigation path and
  event listener rather than adding a second IPC contract.
- Harder: menu IDs are now a two-way contract (`app.nav.*` versus
  `tray.nav.*`) resolved in one handler. Adding a destination means touching
  the menu builder, the tray menu, and the web route type.
- Risk: Linux installs the menu on the main window after that window exists, so
  a future change that recreates the main window must reinstall the menu.
  macOS app-wide menus also appear while the main window is hidden, which is
  intended: Navigate still reveals the window.
- The module is coverage-excluded alongside `tray.rs` because it needs a live
  desktop session.
