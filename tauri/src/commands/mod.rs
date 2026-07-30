//! Tauri commands exposed to the webview.
//!
//! Each command lives in its own submodule. The `collect_commands!`
//! macro drives the tauri-specta Builder that wires the invoke
//! handler in `lib.rs`.

pub mod edition;
pub mod greet;
pub mod remote_search;
pub mod search;
pub mod window;
