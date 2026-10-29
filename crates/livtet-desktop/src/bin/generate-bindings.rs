//! Generates `web/lib/bindings.ts` from the Specta builder
//!
//! This binary exists so `cargo run --bin generate-bindings` (or
//! `cargo test -p livtet-desktop generate_bindings`) can produce
//! the TypeScript bindings without booting the Tauri GUI.
//!
//! The command list lives in [`livtet_desktop_lib::specta_builder`], which is
//! the same builder `run` installs as its invoke handler — so a command cannot
//! be callable at runtime yet absent from `bindings.ts` (or the reverse).

use camino::Utf8PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bindings_path =
        Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/lib/bindings.ts");
    // `bindings.ts` is gitignored and may not exist in a fresh worktree yet;
    // fall back to the uncanonicalized path and let the export create it.
    let bindings_path = bindings_path.canonicalize_utf8().unwrap_or(bindings_path);

    let lang_opts = specta_typescript::Typescript::default();
    livtet_desktop_lib::specta_builder().export(lang_opts, &bindings_path)?;

    println!("bindings.ts written to disk at {bindings_path:?}");
    Ok(())
}
