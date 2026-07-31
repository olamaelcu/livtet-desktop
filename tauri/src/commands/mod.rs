//! Tauri commands exposed to the webview.
//!
//! Each command lives in its own submodule. The `collect_commands!`
//! macro drives the tauri-specta Builder that wires the invoke
//! handler in `lib.rs`.

pub mod diagnostics;
pub mod digital_inventory;
pub mod edition;
pub mod edition_authors;
pub mod edition_identifiers;
pub mod import_edition;
pub mod keyring;
pub mod reindex;
pub mod remote_search;
pub mod search;
pub mod window;
