//! Generates `web/lib/bindings.ts` from the specta Builder.
//!
//! This binary exists so `cargo run --bin generate-bindings` (or
//! `cargo test -p livtet-desktop generate_bindings`) can produce
//! the TypeScript bindings without booting the Tauri GUI. The Tauri
//! runtime export (gated on `#[cfg(debug_assertions)]` in `run()`)
//! produces the same file at runtime — keep the two paths in sync.

use std::path::PathBuf;

use miette::IntoDiagnostic;

fn main() -> miette::Result<()> {
    let specta_builder = livtet_desktop_lib::specta();

    let bindings_path = PathBuf::new()
        .join(env!("CARGO_MANIFEST_DIR"))
        .join("../web/lib/bindings.ts");

    specta_builder
        .export(specta_typescript::Typescript::default(), &bindings_path)
        .into_diagnostic()?;

    println!("bindings.ts written to disk at {bindings_path:?}");
    Ok(())
}
