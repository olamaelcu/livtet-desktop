//! Settings for the KOReader progress sync server.
//!
//! The runtime, the settings store, and the route table live in
//! [`crate::koreader_sync`]; these are the IPC wrappers the Settings panel
//! calls, matching the OPDS server's commands one for one.

use tauri::State;

use crate::koreader_sync::{
    KoreaderSyncDeps, KoreaderSyncError, KoreaderSyncSettings, KoreaderSyncStatus,
};
use crate::types::AppState;

/// The stored configuration plus whether a listener is bound right now.
#[tauri::command]
#[specta::specta]
pub async fn koreader_sync_status(
    state: State<'_, AppState>,
) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
    let deps = KoreaderSyncDeps::from_state(&state);
    Ok(state.koreader_sync.status(&deps).await)
}

/// Persist the configuration and reconcile the listener with it.
///
/// A `password` in the payload is hashed before it is stored; leaving it out
/// keeps the stored one. Idempotent: saving what the listener already runs
/// leaves the bound socket, and any connected reading device, alone.
#[tauri::command]
#[specta::specta]
pub async fn koreader_sync_settings_save(
    settings: KoreaderSyncSettings,
    state: State<'_, AppState>,
) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
    let deps = KoreaderSyncDeps::from_state(&state);
    state.koreader_sync.save(settings, &deps).await
}

/// Bind the listener at the stored port, and remember that it is on.
#[tauri::command]
#[specta::specta]
pub async fn koreader_sync_start(
    state: State<'_, AppState>,
) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
    let deps = KoreaderSyncDeps::from_state(&state);
    state.koreader_sync.start(&deps).await
}

/// Unbind the listener, and remember that it is off.
#[tauri::command]
#[specta::specta]
pub async fn koreader_sync_stop(
    state: State<'_, AppState>,
) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
    let deps = KoreaderSyncDeps::from_state(&state);
    state.koreader_sync.stop(&deps).await
}
