//! Generates `web/lib/bindings.ts` from the specta Builder
//!
//! This binary exists so `cargo run --bin generate-bindings` (or
//! `cargo test -p livtet-desktop generate_bindings`) can produce
//! the TypeScript bindings without booting the Tauri GUI.

use camino::Utf8PathBuf;

use tauri_specta::{Builder, collect_commands};

fn main() {
    let bindings_path =
        Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/lib/bindings.ts");
    // `bindings.ts` is gitignored and may not exist in a fresh worktree yet;
    // fall back to the uncanonicalized path and let the export create it.
    let bindings_path = bindings_path.canonicalize_utf8().unwrap_or(bindings_path);

    let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        livtet_desktop_lib::commands::catalog::get_edition_detail,
        livtet_desktop_lib::commands::catalog::get_edition_covers,
        livtet_desktop_lib::commands::search::search_editions,
        livtet_desktop_lib::commands::search::search_typeahead,
        livtet_desktop_lib::commands::search::search_editions_count,
        livtet_desktop_lib::commands::search::filter_options,
        livtet_desktop_lib::commands::bulk::delete_editions,
        livtet_desktop_lib::commands::bulk::export_editions_csv,
        livtet_desktop_lib::commands::bulk::add_edition_tags,
        livtet_desktop_lib::commands::bulk::remove_edition_tags,
        livtet_desktop_lib::commands::bulk::matching_edition_ids,
        livtet_desktop_lib::commands::import::import_file,
        livtet_desktop_lib::commands::import::import_files,
        livtet_desktop_lib::commands::import::relink_edition_file,
        livtet_desktop_lib::commands::plugins::list_plugins,
        livtet_desktop_lib::commands::opds::opds_default_catalogs,
        livtet_desktop_lib::commands::opds::opds_catalogs_list,
        livtet_desktop_lib::commands::opds::opds_catalogs_create,
        livtet_desktop_lib::commands::opds::opds_catalogs_update,
        livtet_desktop_lib::commands::opds::opds_catalogs_remove,
        livtet_desktop_lib::commands::opds::opds_catalogs_test,
        livtet_desktop_lib::commands::opds::opds_feed,
        livtet_desktop_lib::commands::opds::opds_page,
        livtet_desktop_lib::commands::opds::opds_search,
        livtet_desktop_lib::commands::opds::opds_acquire,
        livtet_desktop_lib::commands::sync::sync_health,
        livtet_desktop_lib::commands::sync::sync_status,
        livtet_desktop_lib::commands::sync::sync_requests_recent,
        livtet_desktop_lib::commands::sync::sync_pairing_begin,
        livtet_desktop_lib::commands::sync::sync_pairing_list,
        livtet_desktop_lib::commands::sync::sync_pairing_approve,
        livtet_desktop_lib::commands::sync::sync_pairing_reject,
        livtet_desktop_lib::commands::sync::sync_devices_list,
        livtet_desktop_lib::commands::sync::sync_devices_revoke,
        livtet_desktop_lib::commands::sync::sync_conflicts_list,
        livtet_desktop_lib::commands::sync::sync_conflicts_resolve,
        livtet_desktop_lib::commands::sync::sync_server_start,
        livtet_desktop_lib::commands::sync::sync_server_stop,
        livtet_desktop_lib::commands::reader::open_reader,
        livtet_desktop_lib::commands::reader::reader_publication,
        livtet_desktop_lib::commands::playback::get_listening_progress,
        livtet_desktop_lib::commands::playback::save_listening_progress,
    ]);

    specta_builder
        .export(specta_typescript::Typescript::default(), &bindings_path)
        .expect("failed to export Specta bindings");

    println!("bindings.ts written to disk at {bindings_path:?}");
}
