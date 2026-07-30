//! Generates `web/lib/bindings.ts` from the specta Builder.
//!
//! This binary exists so `cargo run --bin generate-bindings` (or
//! `cargo test -p livtet-desktop generate_bindings`) can produce
//! the TypeScript bindings without booting the Tauri GUI. The Tauri
//! runtime export (gated on `#[cfg(debug_assertions)]` in `run()`)
//! produces the same file at runtime — keep the two paths in sync.

fn main() {
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(
        tauri_specta::collect_commands![
            livtet_desktop_lib::_bindings_export::greet::greet,
            livtet_desktop_lib::_bindings_export::window::sync_window_title,
            livtet_desktop_lib::_bindings_export::search::search,
            livtet_desktop_lib::_bindings_export::edition::find_edition_by_id,
            livtet_desktop_lib::_bindings_export::edition::find_edition_by_identifier,
        ],
    );

    specta_builder
        .export(
            specta_typescript::Typescript::default(),
            "web/lib/bindings.ts",
        )
        .expect("failed to export TS bindings");

    println!("bindings.ts written to web/lib/bindings.ts");
}
