//! Sync daemon commands: thin proxies over the sidecar's JSON-RPC methods.
//!
//! Each command forwards to [`crate::sync::SyncHandle::call`] and deserialises
//! the daemon's `result` value into a Specta-derivable struct. Integer command
//! parameters are `i32` (the Specta/JS-safe width) and are widened to the
//! daemon's own types when the request params are built.

use livtet_sync_server::rpc::method;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::error::SyncError;
use crate::types::AppState;

/// Deserialise a JSON-RPC `result` into a command DTO.
fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T, SyncError> {
    serde_json::from_value(value)
        .map_err(|error| SyncError::protocol(format!("unexpected sync daemon payload: {error}")))
}

/// Read the `{ "ok": bool }` acknowledgement shared by the mutation methods.
fn ok(value: &serde_json::Value) -> bool {
    value
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_health(state: State<'_, AppState>) -> Result<SyncHealth, SyncError> {
    let value = state
        .sync
        .call(method::HEALTH, serde_json::Value::Null)
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatusInfo, SyncError> {
    let value = state
        .sync
        .call(method::STATUS, serde_json::Value::Null)
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_requests_recent(
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<Vec<SyncRequestRecord>, SyncError> {
    let value = state
        .sync
        .call(
            method::REQUESTS_RECENT,
            serde_json::json!({ "limit": limit }),
        )
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_begin(
    device_type: Option<String>,
    ttl_secs: Option<i32>,
    state: State<'_, AppState>,
) -> Result<SyncPairingTicket, SyncError> {
    let value = state
        .sync
        .call(
            method::PAIRING_BEGIN,
            serde_json::json!({ "device_type": device_type, "ttl_secs": ttl_secs }),
        )
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_list(
    state: State<'_, AppState>,
) -> Result<Vec<SyncPendingPairing>, SyncError> {
    let value = state
        .sync
        .call(method::PAIRING_LIST, serde_json::Value::Null)
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_approve(
    token: String,
    state: State<'_, AppState>,
) -> Result<SyncPairingApproved, SyncError> {
    let value = state
        .sync
        .call(
            method::PAIRING_APPROVE,
            serde_json::json!({ "token": token }),
        )
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_reject(
    token: String,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    let value = state
        .sync
        .call(
            method::PAIRING_REJECT,
            serde_json::json!({ "token": token }),
        )
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_devices_list(state: State<'_, AppState>) -> Result<Vec<SyncDevice>, SyncError> {
    let value = state
        .sync
        .call(method::DEVICES_LIST, serde_json::Value::Null)
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_devices_revoke(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    let value = state
        .sync
        .call(
            method::DEVICES_REVOKE,
            serde_json::json!({ "device_id": device_id }),
        )
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_conflicts_list(
    state: State<'_, AppState>,
) -> Result<Vec<SyncConflict>, SyncError> {
    let value = state
        .sync
        .call(method::CONFLICTS_LIST, serde_json::Value::Null)
        .await?;
    parse(value)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_conflicts_resolve(
    id: i32,
    resolution: String,
    merged_payload: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    let value = state
        .sync
        .call(
            method::CONFLICTS_RESOLVE,
            serde_json::json!({
                "id": id,
                "resolution": resolution,
                "merged_payload": merged_payload,
            }),
        )
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_server_start(state: State<'_, AppState>) -> Result<bool, SyncError> {
    let value = state
        .sync
        .call(method::SERVER_START, serde_json::Value::Null)
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_server_stop(state: State<'_, AppState>) -> Result<bool, SyncError> {
    let value = state
        .sync
        .call(method::SERVER_STOP, serde_json::Value::Null)
        .await?;
    Ok(ok(&value))
}

/// Liveness payload returned by `health`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncHealth {
    pub status: String,
    pub version: String,
    pub pid: i32,
    pub uptime_secs: i32,
    pub server_running: bool,
    pub port: i32,
}

/// Aggregate daemon/server status returned by `status`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncStatusInfo {
    pub device_id: String,
    pub server_running: bool,
    pub host: String,
    pub port: i32,
    pub latest_version: i32,
    pub paired_device_count: i32,
    pub pending_pairing_count: i32,
    pub requests_served: i32,
    pub last_request_at: Option<String>,
}

/// One served HTTP request from `requests.recent`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncRequestRecord {
    pub method: String,
    pub path: String,
    pub status: i32,
    pub at: String,
    pub device_id: Option<String>,
}

/// A freshly minted pairing ticket from `pairing.begin`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPairingTicket {
    pub token: String,
    pub uri: String,
    pub expires_at: String,
}

/// An outstanding pairing from `pairing.list`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPendingPairing {
    pub token: String,
    pub desktop_id: String,
    pub listen_on: Option<String>,
    pub status_id: Option<String>,
    pub device_name: Option<String>,
    pub device_type_id: Option<String>,
    pub device_id: Option<String>,
    pub created_at: String,
    pub expires_at: String,
}

/// The device + session minted by `pairing.approve`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPairingApproved {
    pub device_id: String,
    pub session_token: String,
}

/// A paired device from `devices.list`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncDevice {
    pub device_id: String,
    pub name: Option<String>,
    pub listen_on: Option<String>,
    pub device_type_id: Option<String>,
    pub paired_at: String,
    pub last_sync_at: Option<String>,
}

/// An unresolved sync conflict from `conflicts.list`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncConflict {
    pub id: i32,
    pub entity_type: String,
    pub entity_id: String,
    pub local_payload: String,
    pub remote_payload: String,
    pub resolved: bool,
    pub resolution: Option<String>,
    pub merged_payload: Option<String>,
    pub detected_at: String,
}
