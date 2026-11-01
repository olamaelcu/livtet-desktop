//! Sync daemon commands: thin proxies over the sidecar's JSON-RPC methods.
//!
//! Each command forwards to [`crate::sync::SyncHandle::call`] and deserialises
//! the daemon's `result` value into a Specta-derivable struct. Integer command
//! parameters are `i32` (the Specta/JS-safe width) and are widened to the
//! daemon's own types when the request params are built. Every command is one
//! call in, one struct out.

use livtet_sync_server::rpc::method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::State;

use crate::error::SyncError;
use crate::sync::SyncHandle;
use crate::sync_settings::normalize_device_name;
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

/// Books paired devices have offered, waiting on the reader's decision.
///
/// The daemon records an offer on pull instead of taking it, so everything
/// the prompt shows travels here: the book does not exist in this library yet.
#[tauri::command]
#[specta::specta]
pub async fn sync_offers_list(
    state: State<'_, AppState>,
) -> Result<Vec<SyncIncomingOffer>, SyncError> {
    sync_offers_list_inner(&state.sync).await
}

async fn sync_offers_list_inner(sync: &SyncHandle) -> Result<Vec<SyncIncomingOffer>, SyncError> {
    parse(sync.call(method::OFFERS_LIST, Value::Null).await?)
}

/// Answer an offer: accepting downloads the book, declining refuses it.
///
/// Either way the daemon tells the sender, and `delivered` says whether it
/// could: a peer that has gone away leaves the answer local.
#[tauri::command]
#[specta::specta]
pub async fn sync_offers_decide(
    offer_id: String,
    accepted: bool,
    state: State<'_, AppState>,
) -> Result<SyncOfferDecision, SyncError> {
    sync_offers_decide_inner(&state.sync, offer_id, accepted).await
}

async fn sync_offers_decide_inner(
    sync: &SyncHandle,
    offer_id: String,
    accepted: bool,
) -> Result<SyncOfferDecision, SyncError> {
    parse(
        sync.call(
            method::OFFERS_DECIDE,
            json!({ "offer_id": offer_id, "accepted": accepted }),
        )
        .await?,
    )
}

/// One inbound offer's cover, as a data URL the webview can render.
///
/// The bytes have not been downloaded — that is what accepting does — so the
/// daemon reaches back to the sender over the same token the pull used.
#[tauri::command]
#[specta::specta]
pub async fn sync_offer_cover(
    offer_id: String,
    state: State<'_, AppState>,
) -> Result<Option<SyncOfferCover>, SyncError> {
    sync_offer_cover_inner(&state.sync, offer_id).await
}

async fn sync_offer_cover_inner(
    sync: &SyncHandle,
    offer_id: String,
) -> Result<Option<SyncOfferCover>, SyncError> {
    parse(
        sync.call(method::OFFERS_COVER, json!({ "offer_id": offer_id }))
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

/// Peers advertising `_livtet-sync._tcp` on the local network.
///
/// Paired devices keep advertising, so this returns them too. That is
/// deliberate: an approved device holds a token *this* desktop issued, which
/// only lets it pull from here — receiving from it needs the reverse claim, and
/// hiding it would leave no way to make one. The panel marks the peers it
/// already has a row for instead of dropping them.
#[tauri::command]
#[specta::specta]
pub async fn sync_nearby_devices(
    state: State<'_, AppState>,
) -> Result<Vec<SyncNearbyPeer>, SyncError> {
    sync_nearby_devices_inner(&state.sync).await
}

async fn sync_nearby_devices_inner(sync: &SyncHandle) -> Result<Vec<SyncNearbyPeer>, SyncError> {
    parse(sync.call(method::DISCOVERY_NEARBY, Value::Null).await?)
}

/// The stored device name; `None` means the daemon generates one.
#[tauri::command]
#[specta::specta]
pub async fn sync_device_name_get(state: State<'_, AppState>) -> Result<Option<String>, SyncError> {
    Ok(state
        .sync_settings
        .read()
        .and_then(|settings| settings.device_name))
}

/// Save the name advertised to nearby devices and apply it to the running
/// daemon. A blank name restores the generated fallback.
#[tauri::command]
#[specta::specta]
pub async fn sync_device_name_save(
    name: Option<String>,
    state: State<'_, AppState>,
) -> Result<Option<String>, SyncError> {
    let name = normalize_device_name(name);
    // Read-modify-write: the name is one preference, and saving it must not
    // reset the LAN toggle (or anything added beside it later).
    let settings = state
        .sync_settings
        .read()
        .unwrap_or_default()
        .with_device_name(name.clone());
    state
        .sync_settings
        .write(&settings)
        .map_err(SyncError::storage)?;

    let applied = state
        .sync
        .call(method::DISCOVERY_RENAME, json!({ "name": name.clone() }))
        .await?;
    Ok(applied
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or(name))
}

/// Offer an edition's file to one paired device.
///
/// The daemon records a `file_offer` change row and touches the work/edition/
/// file rows so their audit triggers emit the catalog metadata; the paired
/// device pulls the file, cover, and record on its own schedule.
#[tauri::command]
#[specta::specta]
pub async fn sync_send_book(
    edition_id: String,
    device_id: String,
    state: State<'_, AppState>,
) -> Result<SyncBookOffer, SyncError> {
    parse(
        state
            .sync
            .call(
                method::DEVICES_OFFER_BOOK,
                json!({ "edition_id": edition_id, "device_id": device_id }),
            )
            .await?,
    )
}

/// Pull one paired device's changes, and with them any books it offered.
///
/// Nothing but this call moves bytes towards this library: the offering device
/// only records an offer in its own change log and waits for the receiver to
/// pull it. Accepting an offer (`offers.decide`) downloads from the peer, but
/// the offer itself never arrives without a pull.
#[tauri::command]
#[specta::specta]
pub async fn sync_pull(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<SyncPullOutcome, SyncError> {
    sync_pull_inner(&state.sync, device_id).await
}

async fn sync_pull_inner(
    sync: &SyncHandle,
    device_id: String,
) -> Result<SyncPullOutcome, SyncError> {
    parse(
        sync.call(method::DEVICES_PULL, json!({ "device_id": device_id }))
            .await?,
    )
}

/// Whether the daemon should accept sync peers from the LAN.
#[tauri::command]
#[specta::specta]
pub async fn sync_lan_get(state: State<'_, AppState>) -> Result<bool, SyncError> {
    Ok(state.sync_settings.read().unwrap_or_default().lan)
}

/// Persist the LAN preference.
///
/// The daemon binds its interface when the sidecar is spawned, so this takes
/// effect at the next launch rather than live.
#[tauri::command]
#[specta::specta]
pub async fn sync_lan_set(enabled: bool, state: State<'_, AppState>) -> Result<bool, SyncError> {
    let mut settings = state.sync_settings.read().unwrap_or_default();
    settings.lan = enabled;
    state
        .sync_settings
        .write(&settings)
        .map_err(SyncError::storage)?;
    Ok(settings.lan)
}

/// Ask a discovered peer to pair with this desktop.
///
/// Returns once the claim is filed; the peer approves it in its own UI and the
/// daemon emits `sync://paired` after storing the token.
#[tauri::command]
#[specta::specta]
pub async fn sync_pair_claim(
    listen_on: String,
    node_id: String,
    device_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), SyncError> {
    state
        .sync
        .call(
            method::PAIRING_CLAIM,
            json!({ "listen_on": listen_on, "node_id": node_id, "device_name": device_name }),
        )
        .await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_approve(
    node_id: String,
    state: State<'_, AppState>,
) -> Result<SyncPairingApproved, SyncError> {
    sync_pairing_approve_inner(&state.sync, &node_id).await
}

async fn sync_pairing_approve_inner(
    sync: &SyncHandle,
    node_id: &str,
) -> Result<SyncPairingApproved, SyncError> {
    // The daemon's `TokenParams` field is still named `token`; its value is now
    // the peer's node identity (see `livtet_sync_server::daemon`).
    parse(
        sync.call(method::PAIRING_APPROVE, json!({ "token": node_id }))
            .await?,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn sync_pairing_reject(
    node_id: String,
    state: State<'_, AppState>,
) -> Result<bool, SyncError> {
    sync_pairing_reject_inner(&state.sync, &node_id).await
}

async fn sync_pairing_reject_inner(sync: &SyncHandle, node_id: &str) -> Result<bool, SyncError> {
    // As with approve, the daemon's parameter key is still `token`, now holding
    // the peer's node identity.
    let value = sync
        .call(method::PAIRING_REJECT, json!({ "token": node_id }))
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

/// A freshly minted pairing identity from `pairing.begin` (identity primitive; replaces token/URI).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPairingTicket {
    pub node_id: String,
    pub expires_at: String,
}

/// An outstanding pairing from `pairing.list` (identity-based; node_id replaces token).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPendingPairing {
    pub node_id: String,
    pub desktop_id: String,
    pub listen_on: Option<String>,
    pub status_id: Option<String>,
    pub device_name: Option<String>,
    pub device_type_id: Option<String>,
    pub device_id: Option<String>,
    pub created_at: String,
    pub expires_at: String,
}

/// One peer heard advertising `_livtet-sync._tcp` on the local network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncNearbyPeer {
    pub node_id: String,
    pub device_id: String,
    pub name: String,
    pub device_type: String,
    pub version: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub port: i32,
}

/// The offer recorded by `devices.offer_book`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncBookOffer {
    pub offer_id: String,
    pub inventory_id: String,
    pub device_id: String,
}

/// A book a paired device offered, waiting on the reader's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncIncomingOffer {
    pub offer_id: String,
    pub from_device_id: String,
    /// The offering device's name, when its pairing row still has one.
    pub from_name: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub has_cover: bool,
    pub received_at: String,
}

/// What the daemon did with an answer to an offer.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncOfferDecision {
    pub offer_id: String,
    pub accepted: bool,
    /// Whether the offering device was reachable to hear about it.
    pub delivered: bool,
    /// Catalog rows written; zero for a declined offer.
    pub applied: i32,
    pub conflicts: i32,
}

/// An offer's cover image, ready to hand to an `<img>`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncOfferCover {
    pub content_type: String,
    pub data_url: String,
}

/// The device + session minted by `pairing.approve` (daemon still returns a session token).
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

/// What one `devices.pull` pass took from a paired device.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SyncPullOutcome {
    pub device_id: String,
    /// Catalog rows written by the pulled changes.
    pub applied: i32,
    /// Books the peer offered, now waiting on the offers list.
    pub offered: i32,
    /// Rows the merge could not apply, left for `conflicts.list`.
    pub conflicts: i32,
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

    fn peer(node_id: &str, device_id: &str) -> SyncNearbyPeer {
        SyncNearbyPeer {
            node_id: node_id.to_string(),
            device_id: device_id.to_string(),
            name: node_id.to_string(),
            device_type: "phone".to_string(),
            version: "0.1.0".to_string(),
            hostname: format!("{node_id}.local"),
            addresses: vec!["192.168.1.40".to_string()],
            port: 4321,
        }
    }

    /// Discovery is passed through untouched — approved peers included.
    ///
    /// An approved peer keeps advertising, and it is exactly the row a reader
    /// needs to claim from the other direction: filtering it out of the
    /// pairing panel would leave no way to make an outbound pairing with a
    /// device that already holds a token from this one.
    #[tokio::test]
    async fn nearby_devices_pass_discovery_through() {
        let paired = peer("node-paired", "dev-2");
        let fresh = peer("node-new", "dev-3");
        let replies = HashMap::from([(
            method::DISCOVERY_NEARBY,
            serde_json::to_value([paired.clone(), fresh.clone()]).unwrap(),
        )]);
        let (sync, recorded) = scripted(move |name| Ok(replies[name].clone()));

        let nearby = sync_nearby_devices_inner(&sync).await.unwrap();

        assert_eq!(nearby, vec![paired, fresh]);
        let sent: Vec<String> = recorded
            .lock()
            .unwrap()
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        assert_eq!(sent, vec![method::DISCOVERY_NEARBY]);
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
                to_json(sync_pairing_approve_inner(sync, "node-approve").await)
            }
            method::PAIRING_REJECT => to_json(sync_pairing_reject_inner(sync, "node-reject").await),
            method::DEVICES_LIST => to_json(sync_devices_list_inner(sync).await),
            method::DEVICES_PULL => to_json(sync_pull_inner(sync, "dev-1".to_string()).await),
            method::DEVICES_REVOKE => to_json(sync_devices_revoke_inner(sync, "dev-9").await),
            method::OFFERS_LIST => to_json(sync_offers_list_inner(sync).await),
            method::OFFERS_DECIDE => {
                to_json(sync_offers_decide_inner(sync, "offer-1".to_string(), true).await)
            }
            method::OFFERS_COVER => {
                to_json(sync_offer_cover_inner(sync, "offer-1".to_string()).await)
            }
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
                    "node_id": "node-1",
                    "expires_at": "2026-10-14T10:05:00Z",
                }),
            ),
            decoded(
                method::PAIRING_LIST,
                Value::Null,
                json!([{
                    "node_id": "node-1", "desktop_id": "desk-1", "listen_on": null,
                    "status_id": "pending", "device_name": "Pixel", "device_type_id": "phone",
                    "device_id": null, "created_at": "2026-10-14T10:00:00Z",
                    "expires_at": "2026-10-14T10:05:00Z",
                }]),
            ),
            decoded(
                method::PAIRING_APPROVE,
                json!({ "token": "node-approve" }),
                json!({ "device_id": "dev-2", "session_token": "sess-abc" }),
            ),
            acknowledged(
                method::PAIRING_REJECT,
                json!({ "token": "node-reject" }),
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
                method::DEVICES_PULL,
                json!({ "device_id": "dev-1" }),
                json!({
                    "device_id": "dev-1", "applied": 4, "offered": 2, "conflicts": 1,
                }),
            ),
            decoded(
                method::OFFERS_LIST,
                Value::Null,
                json!([{
                    "offer_id": "offer-1", "from_device_id": "dev-2",
                    "from_name": "Pixel", "title": "Held Book",
                    "authors": ["Ada Lovelace"], "has_cover": true,
                    "received_at": "2026-11-01T09:00:00Z",
                }]),
            ),
            decoded(
                method::OFFERS_DECIDE,
                json!({ "offer_id": "offer-1", "accepted": true }),
                json!({
                    "offer_id": "offer-1", "accepted": true, "delivered": true,
                    "applied": 1, "conflicts": 0,
                }),
            ),
            decoded(
                method::OFFERS_COVER,
                json!({ "offer_id": "offer-1" }),
                json!({ "content_type": "image/png", "data_url": "data:image/png;base64,AA" }),
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
