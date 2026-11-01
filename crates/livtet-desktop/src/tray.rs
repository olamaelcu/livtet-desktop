//! System tray icon and menu for macOS and Linux.
//!
//! The tray keeps the app reachable while its main window is hidden. Its menu
//! mirrors the in-app navigation rail, toggles the main window, and owns the
//! quit action that the hidden window no longer exposes. Linux indicators only
//! surface when a menu is attached, so every action lives in the menu; macOS
//! marks the icon as a template so it follows the menu bar's appearance.

use tauri::{
    AppHandle, Emitter, Manager, Wry,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

/// Event the web layer listens on to follow navigation from native chrome
/// (tray and application menu). The payload is the route to open; it mirrors
/// `web/lib/layout/navItems.ts`.
pub const NAVIGATE_EVENT: &str = "app://navigate";

pub(crate) const MAIN_WINDOW: &str = "main";
pub(crate) const SHOW_WINDOW_ID: &str = "tray.show-window";
pub(crate) const QUIT_ID: &str = "tray.quit";
pub(crate) const NAV_PREFIX: &str = "tray.nav.";
const LIBRARY_ID: &str = "tray.nav.library";
const CATALOG_ID: &str = "tray.nav.catalog";
const PLUGINS_ID: &str = "tray.nav.plugins";
const SETTINGS_ID: &str = "tray.nav.settings";

/// The menu item whose check state reports whether the main window is shown.
struct TrayState {
    show_window: CheckMenuItem<Wry>,
}

/// Builds the tray icon and menu. Call once during app setup.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let show_window =
        CheckMenuItem::with_id(app, SHOW_WINDOW_ID, "Show Window", true, true, None::<&str>)?;
    let library = MenuItem::with_id(app, LIBRARY_ID, "Library", true, None::<&str>)?;
    let catalog = MenuItem::with_id(app, CATALOG_ID, "Catalogs", true, None::<&str>)?;
    let plugins = MenuItem::with_id(app, PLUGINS_ID, "Plugins", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS_ID, "Settings", true, None::<&str>)?;
    let nav_separator = PredefinedMenuItem::separator(app)?;
    let quit_separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT_ID, "Quit Livtet", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_window,
            &nav_separator,
            &library,
            &catalog,
            &plugins,
            &settings,
            &quit_separator,
            &quit,
        ],
    )?;

    // The window-event handler reads the toggle back to keep it in sync when
    // the window is hidden or shown outside the menu.
    app.manage(TrayState { show_window });

    let builder = TrayIconBuilder::with_id("livtet")
        .menu(&menu)
        .tooltip("Livtet");

    let builder = if let Some(icon) = tray_icon() {
        let builder = builder.icon(icon);
        // macOS tints template images for the menu bar's light/dark mode.
        #[cfg(target_os = "macos")]
        let builder = builder.icon_as_template(true);
        builder
    } else if let Some(icon) = app.default_window_icon() {
        builder.icon(icon.clone())
    } else {
        builder
    };

    builder.build(app)?;
    Ok(())
}

/// The tray image. Linux panels draw it untouched, so it carries its own
/// contrast; macOS treats the glyph as a template and recolors it.
fn tray_icon() -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "linux")]
    let bytes: &'static [u8] = include_bytes!("../icons/tray-icon.png");
    #[cfg(target_os = "macos")]
    let bytes: &'static [u8] = include_bytes!("../icons/32x32.png");

    tauri::image::Image::from_bytes(bytes).ok()
}

/// Reflects the main window's visibility in the tray toggle. Called whenever
/// the window is shown or hidden outside the menu itself.
pub fn sync_visibility(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let visible = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if let Err(error) = state.show_window.set_checked(visible) {
        tracing::warn!(%error, "could not update the tray window toggle");
    }
}

/// Shows the main window and routes its webview. Shared by the tray and the
/// application menu so both surfaces follow the same navigation contract.
pub(crate) fn open_route(app: &AppHandle, route: &'static str) {
    focus_main_window(app);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW)
        && let Err(error) = window.emit_to(
            tauri::EventTarget::webview_window(MAIN_WINDOW),
            NAVIGATE_EVENT,
            route,
        )
    {
        tracing::warn!(%error, %route, "could not deliver native navigation");
    }
}

pub(crate) fn route_for_tray_menu_id(id: &str) -> Option<&'static str> {
    route_for_slug(id.strip_prefix(NAV_PREFIX)?)
}

/// Maps the trailing slug of a `*.nav.<slug>` menu id to its route.
pub(crate) fn route_for_slug(slug: &str) -> Option<&'static str> {
    match slug {
        "library" => Some("/library"),
        "catalog" => Some("/catalog"),
        "plugins" => Some("/plugins"),
        "settings" => Some("/settings"),
        _ => None,
    }
}

pub(crate) fn toggle_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    // Read visibility instead of the check state: platforms toggle the item
    // before emitting the event, so the state already equals the old value.
    let visible = window.is_visible().unwrap_or(false);
    let result = if visible {
        window.hide()
    } else {
        window.show().and_then(|()| window.set_focus())
    };
    if let Err(error) = result {
        tracing::warn!(%error, "could not toggle the main window from the tray");
    }
    sync_visibility(app);
}

/// Shows, unminimizes, and focuses the main window. Also used when macOS
/// reopens the app from the Dock while every window is hidden.
pub fn focus_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.unminimize();
    if let Err(error) = window.show().and_then(|()| window.set_focus()) {
        tracing::warn!(%error, "could not show the main window from the tray");
    }
    sync_visibility(app);
}
