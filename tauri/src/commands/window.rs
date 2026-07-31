//! Window chrome sync command.
//!
//! `sync_window_title` takes a title string from the frontend and
//! sets it on the OS window via `WebviewWindow::set_title`. Used by
//! `+layout.svelte`'s `$effect` to mirror `document.title` into the
//! macOS traffic-light / Linux title-bar chrome.

#[tauri::command]
#[specta::specta]
pub async fn sync_window_title(
    window: tauri::WebviewWindow,
    new_title: String,
) -> Result<(), String> {
    window.set_title(&new_title).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    // Window-level behavior is exercised manually (the layout $effect
    // updates the title in pnpm tauri dev). Pure-Rust coverage would
    // require a Tauri test harness, which is out of scope for v1.
}
