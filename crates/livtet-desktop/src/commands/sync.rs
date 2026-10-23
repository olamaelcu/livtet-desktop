//! Sync daemon commands: thin proxies over the sidecar's JSON-RPC methods.
//!
//! Each command forwards to [`crate::sync::SyncHandle::call`] and deserialises
//! the daemon's `result` value into a Specta-derivable struct. Integer command
//! parameters are `i32` (the Specta/JS-safe width) and are widened to the
//! daemon's own types when the request params are built.

use livtet_sync_server::rpc::method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::State;

use crate::error::SyncError;
use crate::sync::SyncHandle;
use crate::types::AppState;

/// Deserialise a JSON-RPC `result` into a command DTO.
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, SyncError> {
    serde_json::from_value(value)
        .map_err(|error| SyncError::protocol(format!("unexpected sync daemon payload: {error}")))
}

/// Read the `{ "ok": bool }` acknowledgement shared by the mutation methods.
fn ok(value: &Value) -> bool {
    value.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_health(state: State<'_, AppState>) -> Result<SyncHealth, SyncError> {
    sync_health_inner(&state.sync).await
}

async fn sync_health_inner(sync: &SyncHandle) -> Result<SyncHealth, SyncError> {
    parse(sync.call(method::HEALTH, Value::Null).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatusInfo, SyncError> {
    sync_status_inner(&state.sync).await
}

async fn sync_status_inner(sync: &SyncHandle) -> Result<SyncStatusInfo, SyncError> {
    parse(sync.call(method::STATUS, Value::Null).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_requests_recent(
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<Vec<SyncRequestRecord>, SyncError> {
    sync_requests_recent_inner(&state.sync, limit).await
}

async fn sync_requests_recent_inner(
    sync: &SyncHandle,
    limit: Option<i32>,
) -> Result<Vec<SyncRequestRecord>, SyncError> {
    parse(
        sync.call(method::REQUESTS_RECENT, json!({ "limit": limit }))
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_begin(
    device_type: Option<String>,
    ttl_secs: Option<i32>,
    state: State<'_, AppState>,
) -> Result<SyncPairingTicket, SyncError> {
    sync_pairing_begin_inner(&state.sync, device_type.as_deref(), ttl_secs).await
}

async fn sync_pairing_begin_inner(
    sync: &SyncHandle,
    device_type: Option<&str>,
    ttl_secs: Option<i32>,
) -> Result<SyncPairingTicket, SyncError> {
    parse(
        sync.call(
            method::PAIRING_BEGIN,
            json!({ "device_type": device_type, "ttl_secs": ttl_secs }),
        )
        .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_list(
    state: State<'_, AppState>,
) -> Result<Vec<SyncPendingPairing>, SyncError> {
    sync_pairing_list_inner(&state.sync).await
}

async fn sync_pairing_list_inner(sync: &SyncHandle) -> Result<Vec<SyncPendingPairing>, SyncError> {
    parse(sync.call(method::PAIRING_LIST, Value::Null).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_approve(
    token: String,
    state: State<'_, AppState>,
) -> Result<SyncPairingApproved, SyncError> {
    sync_pairing_approve_inner(&state.sync, &token).await
}

async fn sync_pairing_approve_inner(
    sync: &SyncHandle,
    token: &str,
) -> Result<SyncPairingApproved, SyncError> {
    parse(
        sync.call(method::PAIRING_APPROVE, json!({ "token": token }))
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_reject(
    token: String,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    sync_pairing_reject_inner(&state.sync, &token).await
}

async fn sync_pairing_reject_inner(sync: &SyncHandle, token: &str) -> Result<bool, SyncError> {
    let value = sync
        .call(method::PAIRING_REJECT, json!({ "token": token }))
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_devices_list(state: State<'_, AppState>) -> Result<Vec<SyncDevice>, SyncError> {
    sync_devices_list_inner(&state.sync).await
}

async fn sync_devices_list_inner(sync: &SyncHandle) -> Result<Vec<SyncDevice>, SyncError> {
    parse(sync.call(method::DEVICES_LIST, Value::Null).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_devices_revoke(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    sync_devices_revoke_inner(&state.sync, &device_id).await
}

async fn sync_devices_revoke_inner(sync: &SyncHandle, device_id: &str) -> Result<bool, SyncError> {
    let value = sync
        .call(method::DEVICES_REVOKE, json!({ "device_id": device_id }))
        .await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_conflicts_list(
    state: State<'_, AppState>,
) -> Result<Vec<SyncConflict>, SyncError> {
    sync_conflicts_list_inner(&state.sync).await
}

async fn sync_conflicts_list_inner(sync: &SyncHandle) -> Result<Vec<SyncConflict>, SyncError> {
    parse(sync.call(method::CONFLICTS_LIST, Value::Null).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn sync_conflicts_resolve(
    id: i32,
    resolution: String,
    merged_payload: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    sync_conflicts_resolve_inner(&state.sync, id, &resolution, merged_payload.as_deref()).await
}

async fn sync_conflicts_resolve_inner(
    sync: &SyncHandle,
    id: i32,
    resolution: &str,
    merged_payload: Option<&str>,
) -> Result<bool, SyncError> {
    let value = sync
        .call(
            method::CONFLICTS_RESOLVE,
            json!({
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
    sync_server_start_inner(&state.sync).await
}

async fn sync_server_start_inner(sync: &SyncHandle) -> Result<bool, SyncError> {
    let value = sync.call(method::SERVER_START, Value::Null).await?;
    Ok(ok(&value))
}

#[tauri::command]
#[specta::specta]
pub async fn sync_server_stop(state: State<'_, AppState>) -> Result<bool, SyncError> {
    sync_server_stop_inner(&state.sync).await
}

async fn sync_server_stop_inner(sync: &SyncHandle) -> Result<bool, SyncError> {
    let value = sync.call(method::SERVER_STOP, Value::Null).await?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::sync::testing::scripted;
    use std::collections::HashMap;

    /// A minimal DTO: `parse` is generic, so its contract is the same whatever
    /// the caller asks for.
    #[derive(Debug, PartialEq, Eq, Deserialize)]
    struct Probe {
        name: String,
        count: i32,
    }

    #[test]
    fn parse_deserializes_a_matching_shape() {
        let parsed: Probe = parse(json!({ "name": "alpha", "count": 3 })).unwrap();

        assert_eq!(
            parsed,
            Probe {
                name: "alpha".to_string(),
                count: 3
            }
        );
    }

    #[test]
    fn parse_maps_a_mismatch_to_a_protocol_error() {
        let error = parse::<Probe>(json!({ "name": "alpha" })).unwrap_err();

        assert!(
            matches!(error, SyncError::Protocol { .. }),
            "expected a protocol error, got {error:?}"
        );
    }

    #[test]
    fn ok_is_true_for_a_success_envelope() {
        assert!(ok(&json!({ "ok": true })));
    }

    /// Anything that is not an explicit `{"ok": true}` reads as failure: a
    /// missing key, a wrong type, or a non-object all default to false rather
    /// than being treated as success.
    #[test]
    fn ok_is_false_for_anything_else() {
        assert!(!ok(&json!({ "ok": false })));
        assert!(!ok(&json!({})));
        assert!(!ok(&json!({ "ok": "yes" })));
        assert!(!ok(&json!(null)));
    }

    /// Run the command behind daemon method `name` with fixed arguments and
    /// normalise its typed result back to JSON, so every command fits one table.
    async fn invoke(sync: &SyncHandle, name: &str) -> Result<Value, SyncError> {
        fn to_json<T: Serialize>(result: Result<T, SyncError>) -> Result<Value, SyncError> {
            result.map(|value| serde_json::to_value(value).unwrap())
        }
        match name {
            method::HEALTH => to_json(sync_health_inner(sync).await),
            method::STATUS => to_json(sync_status_inner(sync).await),
            method::REQUESTS_RECENT => to_json(sync_requests_recent_inner(sync, Some(5)).await),
            method::PAIRING_BEGIN => {
                to_json(sync_pairing_begin_inner(sync, Some("phone"), None).await)
            }
            method::PAIRING_LIST => to_json(sync_pairing_list_inner(sync).await),
            method::PAIRING_APPROVE => {
                to_json(sync_pairing_approve_inner(sync, "tok-approve").await)
            }
            method::PAIRING_REJECT => to_json(sync_pairing_reject_inner(sync, "tok-reject").await),
            method::DEVICES_LIST => to_json(sync_devices_list_inner(sync).await),
            method::DEVICES_REVOKE => to_json(sync_devices_revoke_inner(sync, "dev-9").await),
            method::CONFLICTS_LIST => to_json(sync_conflicts_list_inner(sync).await),
            method::CONFLICTS_RESOLVE => to_json(
                sync_conflicts_resolve_inner(sync, 7, "merge", Some(r#"{"title":"x"}"#)).await,
            ),
            method::SERVER_START => to_json(sync_server_start_inner(sync).await),
            method::SERVER_STOP => to_json(sync_server_stop_inner(sync).await),
            other => panic!("no command for daemon method {other}"),
        }
    }

    /// One command's contract with the daemon: the request it must send for
    /// the arguments [`invoke`] passes, a daemon reply, and what the command
    /// must return for that reply.
    struct Case {
        method: &'static str,
        params: Value,
        reply: Value,
        returns: Value,
    }

    /// Decoding commands must hand back exactly what the daemon sent.
    fn decoded(method: &'static str, params: Value, reply: Value) -> Case {
        Case {
            method,
            params,
            returns: reply.clone(),
            reply,
        }
    }

    fn acknowledged(method: &'static str, params: Value, reply: Value, returns: bool) -> Case {
        Case {
            method,
            params,
            reply,
            returns: json!(returns),
        }
    }

    fn cases() -> Vec<Case> {
        vec![
            decoded(
                method::HEALTH,
                Value::Null,
                json!({
                    "status": "ok", "version": "0.1.0", "pid": 4242,
                    "uptime_secs": 17, "server_running": true, "port": 8443,
                }),
            ),
            decoded(
                method::STATUS,
                Value::Null,
                json!({
                    "device_id": "desk-1", "server_running": false, "host": "127.0.0.1",
                    "port": 0, "latest_version": 12, "paired_device_count": 2,
                    "pending_pairing_count": 1, "requests_served": 30, "last_request_at": null,
                }),
            ),
            decoded(
                method::REQUESTS_RECENT,
                json!({ "limit": 5 }),
                json!([
                    { "method": "GET", "path": "/sync/pull", "status": 200,
                      "at": "2026-10-14T10:00:00Z", "device_id": "dev-1" },
                    { "method": "POST", "path": "/pair", "status": 403,
                      "at": "2026-10-14T10:01:00Z", "device_id": null },
                ]),
            ),
            decoded(
                method::PAIRING_BEGIN,
                json!({ "device_type": "phone", "ttl_secs": null }),
                json!({
                    "token": "tok-1", "uri": "livtet://pair?token=tok-1",
                    "expires_at": "2026-10-14T10:05:00Z",
                }),
            ),
            decoded(
                method::PAIRING_LIST,
                Value::Null,
                json!([{
                    "token": "tok-1", "desktop_id": "desk-1", "listen_on": null,
                    "status_id": "pending", "device_name": "Pixel", "device_type_id": "phone",
                    "device_id": null, "created_at": "2026-10-14T10:00:00Z",
                    "expires_at": "2026-10-14T10:05:00Z",
                }]),
            ),
            decoded(
                method::PAIRING_APPROVE,
                json!({ "token": "tok-approve" }),
                json!({ "device_id": "dev-2", "session_token": "sess-abc" }),
            ),
            acknowledged(
                method::PAIRING_REJECT,
                json!({ "token": "tok-reject" }),
                json!({ "ok": true }),
                true,
            ),
            decoded(
                method::DEVICES_LIST,
                Value::Null,
                json!([{
                    "device_id": "dev-2", "name": "Pixel", "listen_on": "192.168.1.20:7000",
                    "device_type_id": "phone", "paired_at": "2026-10-14T10:02:00Z",
                    "last_sync_at": null,
                }]),
            ),
            acknowledged(
                method::DEVICES_REVOKE,
                json!({ "device_id": "dev-9" }),
                json!({ "ok": true, "removed": 1 }),
                true,
            ),
            decoded(
                method::CONFLICTS_LIST,
                Value::Null,
                json!([{
                    "id": 3, "entity_type": "work", "entity_id": "w-1",
                    "local_payload": "{\"a\":1}", "remote_payload": "{\"a\":2}",
                    "resolved": false, "resolution": null, "merged_payload": null,
                    "detected_at": "2026-10-14T09:00:00Z",
                }]),
            ),
            acknowledged(
                method::CONFLICTS_RESOLVE,
                json!({ "id": 7, "resolution": "merge", "merged_payload": r#"{"title":"x"}"# }),
                json!({ "ok": false }),
                false,
            ),
            acknowledged(
                method::SERVER_START,
                Value::Null,
                json!({ "ok": true, "already_running": true }),
                true,
            ),
            acknowledged(
                method::SERVER_STOP,
                Value::Null,
                json!({ "ok": false }),
                false,
            ),
        ]
    }

    #[tokio::test]
    async fn every_command_sends_its_request_and_returns_the_decoded_reply() {
        let table = cases();
        let replies: HashMap<&'static str, Value> = table
            .iter()
            .map(|case| (case.method, case.reply.clone()))
            .collect();
        let (sync, recorded) = scripted(move |name| Ok(replies[name].clone()));

        for case in &table {
            let returned = invoke(&sync, case.method)
                .await
                .unwrap_or_else(|error| panic!("{}: {error:?}", case.method));
            assert_eq!(returned, case.returns, "{} result", case.method);
        }

        let sent = recorded.lock().unwrap().clone();
        let expected: Vec<(String, Value)> = table
            .iter()
            .map(|case| (case.method.to_string(), case.params.clone()))
            .collect();
        assert_eq!(sent, expected);
    }

    #[tokio::test]
    async fn decoding_commands_reject_a_reply_of_the_wrong_shape() {
        let (sync, _) = scripted(|_| Ok(json!({ "unexpected": "shape" })));

        // Decoding commands are the ones whose expected return is the reply itself.
        for case in cases().iter().filter(|case| case.returns == case.reply) {
            let result = invoke(&sync, case.method).await;
            assert!(
                matches!(result, Err(SyncError::Protocol { .. })),
                "{}: expected a protocol error, got {result:?}",
                case.method
            );
        }
    }

    /// A daemon error must surface as an error from every command, including
    /// the acknowledgement ones that would otherwise read a missing `ok` as
    /// `false`.
    #[tokio::test]
    async fn every_command_surfaces_a_daemon_error() {
        let (sync, _) = scripted(|_| Err("database is locked".to_string()));

        for case in cases() {
            match invoke(&sync, case.method).await {
                Err(SyncError::Rpc { message }) => {
                    assert_eq!(message, "database is locked", "{}", case.method);
                }
                other => panic!("{}: expected an rpc error, got {other:?}", case.method),
            }
        }
    }
}
