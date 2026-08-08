//! Specta command bindings for TypeScript code generation.
//!
//! Commands are kept in a single `tauri_specta::collect_commands!` macro
//! because `tauri::generate_handler!` closures consume the `Invoke`
//! value and there is no public API to merge multiple handler closures
//! (see tauri-apps/tauri#15597).

use crate::commands;

#[doc(hidden)]
pub use crate::commands::covers;
#[doc(hidden)]
pub use crate::commands::diagnostics;
#[doc(hidden)]
pub use crate::commands::digital_inventory;
#[doc(hidden)]
pub use crate::commands::edition;
#[doc(hidden)]
pub use crate::commands::fonts;
#[doc(hidden)]
pub use crate::commands::edition_authors;
#[doc(hidden)]
pub use crate::commands::edition_identifiers;
#[doc(hidden)]
pub use crate::commands::import_edition;
#[doc(hidden)]
pub use crate::commands::keyring;
#[doc(hidden)]
pub use crate::commands::language_preference;
#[doc(hidden)]
pub use crate::commands::plugins;
#[doc(hidden)]
pub use crate::commands::reindex;
#[doc(hidden)]
pub use crate::commands::remote_search;
#[doc(hidden)]
pub use crate::commands::search;
#[doc(hidden)]
pub use crate::commands::window;
#[doc(hidden)]
pub use crate::state;

pub fn specta() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::window::sync_window_title,
            commands::search::search,
            commands::edition::find_edition_by_id,
            commands::edition::find_edition_by_identifier,
            commands::digital_inventory::find_files_by_edition,
            commands::digital_inventory::add_digital_inventory,
            commands::digital_inventory::remove_book,
            commands::edition_authors::find_authors_by_edition,
            commands::edition_identifiers::find_identifiers_by_edition,
            commands::remote_search::remote_search,
            commands::remote_search::cancel_remote_search,
            commands::keyring::get_hardcover_key,
            commands::keyring::set_hardcover_key,
            commands::keyring::clear_hardcover_key,
            commands::keyring::verify_hardcover_key,
            commands::language_preference::get_language_preference,
            commands::language_preference::set_language_preference,
            commands::import_edition::import_edition,
            commands::diagnostics::export_logs,
            commands::reindex::reindex,
            commands::covers::fetch_cover,
            commands::covers::list_covers,
            commands::fonts::download_font,
            commands::fonts::delete_font,
            commands::fonts::list_downloaded_fonts,
            commands::plugins::list_plugins,
            commands::plugins::load_plugin,
            commands::plugins::unload_plugin,
            commands::plugins::set_plugin_disabled,
            commands::plugins::call_plugin_capability,
            commands::plugins::get_plugin_setting,
            commands::plugins::write_plugin_setting,
        ])
        .events(tauri_specta::collect_events![
            crate::commands::remote_search::chain::ProviderFailureEvent,
            crate::commands::reindex::ReindexComplete,
        ])
}