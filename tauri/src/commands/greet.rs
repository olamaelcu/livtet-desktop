//! Existing greet command, annotated with `#[specta::specta]`.
//! The Builder switchover happens in Phase 2.

#[tauri::command]
#[specta::specta]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}
