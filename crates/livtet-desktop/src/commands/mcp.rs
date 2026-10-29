//! Settings for the localhost-only MCP endpoint (ADR-0041).
//!
//! The runtime, the settings store, and the command bridge live in
//! [`crate::mcp`]; these are the IPC wrappers the Settings panel calls.

use tauri::State;

use crate::mcp::{CommandBridge, McpSettings, McpSettingsError, McpStatus};
use crate::types::AppState;

/// The stored MCP settings plus whether the endpoint is listening right now.
///
/// `env_port` reports a `LIVTET_MCP_PORT` override, which wins over the stored
/// port and can switch the endpoint on; the panel shows it so the displayed
/// state matches what is actually bound.
#[tauri::command]
#[specta::specta]
pub async fn mcp_settings_get(state: State<'_, AppState>) -> Result<McpStatus, McpSettingsError> {
    Ok(state.mcp.status().await)
}

/// Persist the MCP settings and reconcile the listener with them.
///
/// Idempotent: saving the configuration the listener already runs leaves the
/// bound socket (and any connected client) alone.
#[tauri::command]
#[specta::specta]
pub async fn mcp_settings_save(
    settings: McpSettings,
    state: State<'_, AppState>,
) -> Result<McpStatus, McpSettingsError> {
    let bridge = CommandBridge::from_state(&state);
    state.mcp.save(settings, bridge).await
}
