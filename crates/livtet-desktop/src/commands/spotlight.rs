//! Settings for macOS Spotlight indexing (ADR-0051).
//!
//! The store, the index writer, and the activation hook live in
//! [`crate::spotlight`]; these are the IPC wrappers the Settings panel calls.

use tauri::State;

use crate::spotlight::{SpotlightError, SpotlightStatus};
use crate::types::AppState;

/// The persisted preference plus whether the platform can index right now.
#[tauri::command]
#[specta::specta]
pub async fn spotlight_status(
    state: State<'_, AppState>,
) -> Result<SpotlightStatus, SpotlightError> {
    Ok(state.spotlight.status())
}

/// Opt in or out. Enabling writes the library; disabling deletes its domain.
#[tauri::command]
#[specta::specta]
pub async fn spotlight_set_enabled(
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<SpotlightStatus, SpotlightError> {
    let db = state.db.db_conn();
    state.spotlight.set_enabled(&db, enabled).await
}

/// Rewrite the library index from the current database.
#[tauri::command]
#[specta::specta]
pub async fn spotlight_reindex(
    state: State<'_, AppState>,
) -> Result<SpotlightStatus, SpotlightError> {
    let db = state.db.db_conn();
    state.spotlight.reindex(&db).await
}
