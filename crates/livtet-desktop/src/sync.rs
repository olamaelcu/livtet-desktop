//! Sidecar bridge to the `livtet-sync-daemon` process.
//!
//! The daemon speaks JSON-RPC 2.0 as newline-delimited JSON over its
//! stdin/stdout pair (see `livtet_sync_server::rpc`). [`SyncHandle`] owns the
//! child process spawned through `tauri-plugin-shell` and multiplexes many
//! concurrent calls over that single pipe pair: every call allocates an id,
//! parks a one-shot sender in the pending registry, and is resolved when the
//! reader task observes the matching response. Daemon notifications are
//! re-emitted as Tauri events so the frontend can react to pairing and sync
//! activity.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use camino::Utf8Path;
use livtet_sync_server::rpc::{self, RpcMessage};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::oneshot;

use crate::error::SyncError;

/// How long a single JSON-RPC call may wait for its response.
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// The daemon sidecar name, as declared in `bundle.externalBin`.
const SIDECAR: &str = "livtet-sync-daemon";

/// The registry of in-flight requests, shared between the handle and the
/// reader task that resolves them.
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// Shared owner of the daemon child process and its in-flight requests.
///
/// `pending` is `Arc`-wrapped so the output-draining task spawned by
/// [`SyncHandle::spawn`] can resolve requests against the same registry the
/// returned handle writes into.
pub struct SyncHandle {
    child: AsyncMutex<CommandChild>,
    pending: Pending,
    next_id: AtomicU64,
}

impl SyncHandle {
    /// Spawn the daemon sidecar against `db_path` and start draining its output.
    ///
    /// The daemon is configured to bind `127.0.0.1:0` (an ephemeral port).
    pub fn spawn(app: &AppHandle, db_path: &Utf8Path) -> Result<Self, SyncError> {
        let command = app
            .shell()
            .sidecar(SIDECAR)
            .map_err(SyncError::unavailable)?
            .args([
                "--db",
                db_path.as_str(),
                "--host",
                "127.0.0.1",
                "--port",
                "0",
            ]);

        let (mut rx, child) = command.spawn().map_err(SyncError::unavailable)?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let reader_app = app.clone();
        let reader_pending = Arc::clone(&pending);
        let _reader = tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => {
                        let line = String::from_utf8_lossy(&bytes);
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        handle_line(&reader_app, &reader_pending, line);
                    }
                    CommandEvent::Stderr(bytes) => {
                        let line = String::from_utf8_lossy(&bytes);
                        let line = line.trim();
                        if !line.is_empty() {
                            tracing::warn!(target: "livtet_sync_daemon", "{line}");
                        }
                    }
                    CommandEvent::Error(error) => {
                        tracing::warn!(error = %error, "sync daemon stream error");
                    }
                    CommandEvent::Terminated(payload) => {
                        tracing::warn!(
                            code = ?payload.code,
                            signal = ?payload.signal,
                            "sync daemon terminated"
                        );
                        fail_pending(&reader_pending, "sync daemon terminated");
                        break;
                    }
                    // `CommandEvent` is `#[non_exhaustive]`.
                    _ => {}
                }
            }
        });

        Ok(Self {
            child: AsyncMutex::new(child),
            pending,
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and await its correlated response.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, SyncError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();

        {
            let mut registry = self
                .pending
                .lock()
                .map_err(|_| SyncError::protocol("sync request registry poisoned"))?;
            registry.insert(id, tx);
        }

        let line = rpc::encode_request(id, method, params);
        let write_result = {
            let mut child = self.child.lock().await;
            child.write(format!("{line}\n").as_bytes())
        };
        if let Err(error) = write_result {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            return Err(SyncError::unavailable(error));
        }

        match tokio::time::timeout(RPC_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(message))) => Err(SyncError::rpc(message)),
            Ok(Err(_)) => Err(SyncError::protocol("sync daemon closed before responding")),
            Err(_) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                Err(SyncError::unavailable(
                    "sync daemon did not respond in time",
                ))
            }
        }
    }

    /// Best-effort request to stop the daemon before the app exits.
    pub async fn shutdown(&self) {
        let _ = self.call(rpc::method::SHUTDOWN, Value::Null).await;
    }
}

/// Decode one NDJSON line, routing responses to their waiter and notifications
/// onto the Tauri event bus.
fn handle_line(app: &AppHandle, pending: &Pending, line: &str) {
    match rpc::parse_message(line) {
        Ok(RpcMessage::Response(response)) => {
            let Some(id) = response.id.as_ref().and_then(Value::as_u64) else {
                tracing::debug!("sync daemon response carries no numeric id");
                return;
            };
            let sender = pending.lock().ok().and_then(|mut map| map.remove(&id));
            let Some(sender) = sender else {
                tracing::debug!(id, "sync daemon response has no matching request");
                return;
            };
            let outcome = match (response.result, response.error) {
                (_, Some(error)) => Err(error.message),
                (Some(result), None) => Ok(result),
                (None, None) => Err("sync daemon returned an empty response".to_string()),
            };
            let _ = sender.send(outcome);
        }
        Ok(RpcMessage::Notification(notification)) => route_notification(app, &notification),
        Ok(RpcMessage::Request(_)) => {
            tracing::debug!("ignoring inbound request from sync daemon");
        }
        Err(error) => {
            tracing::warn!(error = %error, "sync daemon emitted an unparsable line");
        }
    }
}

/// Map a daemon notification method onto a `sync://…` Tauri event.
fn route_notification(app: &AppHandle, notification: &rpc::RpcNotification) {
    let event = match notification.method.as_str() {
        rpc::method::NOTIFY_PAIRING_REQUESTED => "sync://pairing-requested",
        rpc::method::NOTIFY_REQUEST_RECEIVED => "sync://request",
        rpc::method::NOTIFY_SYNC_COMPLETED => "sync://completed",
        rpc::method::NOTIFY_SERVER_STARTED => "sync://server-started",
        rpc::method::NOTIFY_SERVER_STOPPED => "sync://server-stopped",
        other => {
            tracing::debug!(method = other, "unhandled sync daemon notification");
            return;
        }
    };

    if let Err(error) = app.emit(event, notification.params.clone()) {
        tracing::warn!(error = %error, event, "failed to emit sync daemon event");
    }
}

/// Resolve every parked request with `message`; used when the child exits.
fn fail_pending(pending: &Pending, message: &str) {
    let drained = pending
        .lock()
        .map(|mut map| std::mem::take(&mut *map))
        .unwrap_or_default();
    for (_, sender) in drained {
        let _ = sender.send(Err(message.to_string()));
    }
}
