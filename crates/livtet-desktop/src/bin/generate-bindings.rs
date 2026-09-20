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
        livtet_desktop_lib::commands::search::search_editions,
        livtet_desktop_lib::commands::search::search_typeahead,
        livtet_desktop_lib::commands::search::search_editions_count,
        livtet_desktop_lib::commands::plugins::list_plugins,
    ]);

    specta_builder
        .export(specta_typescript::Typescript::default(), &bindings_path)
        .expect("failed to export Specta bindings");

    println!("bindings.ts written to disk at {bindings_path:?}");
}
