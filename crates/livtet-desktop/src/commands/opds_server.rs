//! Settings for the OPDS server that publishes the library to e-readers
//! (ADR-0045).
//!
//! The runtime, the settings store, and the dependency set live in
//! [`crate::opds_server`]; these are the IPC wrappers the Settings panel calls.

use tauri::State;

use crate::opds_server::{OpdsServerDeps, OpdsServerError, OpdsServerSettings, OpdsServerStatus};
use crate::types::AppState;

/// The stored OPDS server settings plus whether a listener is bound right now.
#[tauri::command]
#[specta::specta]
pub async fn opds_server_status(
    state: State<'_, AppState>,
) -> Result<OpdsServerStatus, OpdsServerError> {
    Ok(state.opds_server.status().await)
}

/// Persist the OPDS server settings and reconcile the listener with them.
///
/// Idempotent: saving the configuration the listener already runs leaves the
/// bound socket (and any connected e-reader) alone.
#[tauri::command]
#[specta::specta]
pub async fn opds_server_settings_save(
    settings: OpdsServerSettings,
    state: State<'_, AppState>,
) -> Result<OpdsServerStatus, OpdsServerError> {
    let deps = OpdsServerDeps::from_state(&state);
    state.opds_server.save(settings, &deps).await
}

/// Bind the listener at the stored port, and remember that it is on.
#[tauri::command]
#[specta::specta]
pub async fn opds_server_start(
    state: State<'_, AppState>,
) -> Result<OpdsServerStatus, OpdsServerError> {
    let deps = OpdsServerDeps::from_state(&state);
    state.opds_server.start(&deps).await
}

/// Unbind the listener, and remember that it is off.
#[tauri::command]
#[specta::specta]
pub async fn opds_server_stop(
    state: State<'_, AppState>,
) -> Result<OpdsServerStatus, OpdsServerError> {
    let deps = OpdsServerDeps::from_state(&state);
    state.opds_server.stop(&deps).await
}
