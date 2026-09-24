//! Generates `web/lib/bindings.ts` from the specta Builder
//!
//! This binary exists so `cargo run --bin generate-bindings` (or
//! `cargo test -p livtet-desktop generate_bindings`) can produce
//! the TypeScript bindings without booting the Tauri GUI.

use camino::Utf8PathBuf;

use tauri_specta::{Builder, collect_commands};

fn main() {
    let bindings_path = Utf8PathBuf::new()
        .join(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/lib/bindings.ts")
        .canonicalize_utf8()
        .expect("Could not determine the path on disk to the bindings.ts location.");

    let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        livtet_desktop_lib::commands::catalog::get_edition_detail,
        livtet_desktop_lib::commands::search::search_editions,
        livtet_desktop_lib::commands::search::search_typeahead,
        livtet_desktop_lib::commands::search::search_editions_count,
        livtet_desktop_lib::commands::search::filter_options,
        livtet_desktop_lib::commands::import::import_file,
        livtet_desktop_lib::commands::import::import_files,
        livtet_desktop_lib::commands::plugins::list_plugins,
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
    ]);

    specta_builder
        .export(specta_typescript::Typescript::default(), &bindings_path)
        .expect("failed to export Specta bindings");

    println!("bindings.ts written to disk at {bindings_path:?}");
}
