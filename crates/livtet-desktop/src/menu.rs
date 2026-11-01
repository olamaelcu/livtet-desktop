//! Native application menu (macOS menu bar, Linux window menu).
//!
//! The menu uses predefined items for roles the platform owns (About, Edit,
//! Quit) and custom items for Livtet navigation. Navigation shares the tray's
//! `app://navigate` contract, so both surfaces route through one web listener.
//!
//! Linux has no app-wide menu because it would also attach to reader windows;
//! the menu is installed on the main window instead. macOS only supports an
//! app-wide menu, so reader windows see it too, which is conventional there.
//!
//! No native accelerators are set: the web layer already owns user-rebindable
//! hotkeys (`web/lib/hotkeys/commands.ts`), and duplicating them in the menu
//! would make one keypress dispatch twice.

use tauri::{
    AppHandle, Manager, Wry,
    menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu},
};

const NAV_PREFIX: &str = "app.nav.";
const LIBRARY_ID: &str = "app.nav.library";
const CATALOG_ID: &str = "app.nav.catalog";
const PLUGINS_ID: &str = "app.nav.plugins";
const SETTINGS_ID: &str = "app.nav.settings";
const QUIT_ID: &str = "app.quit";

/// Builds and installs the menu: app-wide on macOS, main-window-scoped on
/// Linux so reader windows keep their chrome-free layout.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = build(app)?;

    #[cfg(target_os = "macos")]
    let _ = app.set_menu(menu)?;

    #[cfg(target_os = "linux")]
    if let Some(window) = app.get_webview_window(crate::tray::MAIN_WINDOW) {
        let _ = window.set_menu(menu)?;
    }

    Ok(())
}

/// Dispatches any native menu event, tray or app menu. Registered once on the
/// builder so an event cannot be handled twice.
pub fn handle_event(app: &AppHandle, id: &str) {
    if id == crate::tray::SHOW_WINDOW_ID {
        crate::tray::toggle_main_window(app);
    } else if id == crate::tray::QUIT_ID || id == QUIT_ID {
        app.exit(0);
    } else if let Some(route) = route_for_menu_id(id) {
        crate::tray::open_route(app, route);
    }
}

fn route_for_menu_id(id: &str) -> Option<&'static str> {
    match id.strip_prefix(NAV_PREFIX) {
        Some(slug) => crate::tray::route_for_slug(slug),
        None => crate::tray::route_for_tray_menu_id(id),
    }
}

fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let about = AboutMetadata {
        name: Some("Livtet".into()),
        version: Some(app.package_info().version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app
            .config()
            .bundle
            .publisher
            .clone()
            .map(|publisher| vec![publisher]),
        ..Default::default()
    };

    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let navigate = Submenu::with_items(
        app,
        "Navigate",
        true,
        &[
            &MenuItem::with_id(app, LIBRARY_ID, "Library", true, None::<&str>)?,
            &MenuItem::with_id(app, CATALOG_ID, "Catalogs", true, None::<&str>)?,
            &MenuItem::with_id(app, PLUGINS_ID, "Plugins", true, None::<&str>)?,
            &MenuItem::with_id(app, SETTINGS_ID, "Settings", true, None::<&str>)?,
        ],
    )?;

    // macOS requires the first submenu to be the application menu and ignores
    // window-scoped menus, so this is the only menu the app installs.
    #[cfg(target_os = "macos")]
    {
        let app_menu = Submenu::with_items(
            app,
            "Livtet",
            true,
            &[
                &PredefinedMenuItem::about(app, None, Some(about))?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::services(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        let window = Submenu::with_items(
            app,
            "Window",
            true,
            &[
                &PredefinedMenuItem::minimize(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::close_window(app, None)?,
            ],
        )?;
        Menu::with_items(app, &[&app_menu, &edit, &navigate, &window])
    }

    // Linux lacks predefined Quit, Close Window, and Minimize, so the quit
    // action is a custom item handled in `handle_event`.
    #[cfg(target_os = "linux")]
    {
        let file = Submenu::with_items(
            app,
            "File",
            true,
            &[&MenuItem::with_id(
                app,
                QUIT_ID,
                "Quit Livtet",
                true,
                None::<&str>,
            )?],
        )?;
        let help = Submenu::with_items(
            app,
            "Help",
            true,
            &[&PredefinedMenuItem::about(app, None, Some(about))?],
        )?;
        Menu::with_items(app, &[&file, &edit, &navigate, &help])
    }
}
