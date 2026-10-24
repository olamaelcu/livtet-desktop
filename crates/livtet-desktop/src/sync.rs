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
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use camino::Utf8Path;
use livtet_sync_server::rpc::{self, RpcMessage};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::{mpsc, oneshot};

use crate::error::SyncError;

/// How long a single JSON-RPC call may wait for its response.
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// The daemon sidecar name, as declared in `bundle.externalBin`.
const SIDECAR: &str = "livtet-sync-daemon";

/// The registry of in-flight requests, shared between the handle and the
/// reader task that resolves them.
type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// The daemon's stdin, as seen by [`SyncHandle::call`].
type Stdin = Box<dyn AsyncWrite + Send + Unpin>;

/// Shared owner of the daemon's stdin and its in-flight requests.
///
/// `pending` is `Arc`-wrapped so the output-draining task started by
/// [`SyncHandle::connect`] can resolve requests against the same registry the
/// handle writes into.
pub struct SyncHandle {
    stdin: AsyncMutex<Stdin>,
    pending: Pending,
    next_id: AtomicU64,
    timeout: Duration,
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

        let (rx, child) = command.spawn().map_err(SyncError::unavailable)?;

        let app = app.clone();
        let emit = move |event: &'static str, payload: Value| {
            if let Err(error) = app.emit(event, payload) {
                tracing::warn!(error = %error, event, "failed to emit sync daemon event");
            }
        };

        Ok(Self::connect(ChildStdin(child), rx, emit, RPC_TIMEOUT))
    }

    /// Wire a handle to a running daemon: requests are written to `stdin`, the
    /// daemon's output arrives on `events`, and recognised notifications are
    /// passed to `emit` as `(event name, params)`.
    fn connect<W, E>(
        stdin: W,
        events: mpsc::Receiver<CommandEvent>,
        emit: E,
        timeout: Duration,
    ) -> Self
    where
        W: AsyncWrite + Send + Unpin + 'static,
        E: Fn(&'static str, Value) + Send + 'static,
    {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        start_reader(Arc::clone(&pending), events, emit);

        Self {
            stdin: AsyncMutex::new(Box::new(stdin)),
            pending,
            next_id: AtomicU64::new(1),
            timeout,
        }
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

        let mut line = rpc::encode_request(id, method, params);
        line.push('\n');
        let write_result = {
            let mut stdin = self.stdin.lock().await;
            match stdin.write_all(line.as_bytes()).await {
                Ok(()) => stdin.flush().await,
                Err(error) => Err(error),
            }
        };
        if let Err(error) = write_result {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            return Err(SyncError::unavailable(error));
        }

        match tokio::time::timeout(self.timeout, rx).await {
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

/// Adapts the sidecar's stdin pipe to [`AsyncWrite`]. The shell plugin only
/// exposes a synchronous write, so each poll completes immediately.
struct ChildStdin(CommandChild);

impl AsyncWrite for ChildStdin {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(
            self.get_mut()
                .0
                .write(buf)
                .map(|()| buf.len())
                .map_err(|error| io::Error::other(error.to_string())),
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Start the output-draining reader task.
///
/// Uses the Tauri async runtime; the returned handle is intentionally dropped
/// to let the task run independently. When the daemon's output ends, for
/// whatever reason, every parked request is failed: no response can arrive
/// any more.
fn start_reader<E>(pending: Pending, mut events: mpsc::Receiver<CommandEvent>, emit: E)
where
    E: Fn(&'static str, Value) + Send + 'static,
{
    drop(tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(bytes) => {
                    let line = String::from_utf8_lossy(&bytes);
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    handle_line(&emit, &pending, line);
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
                    break;
                }
                // `CommandEvent` is `#[non_exhaustive]`.
                _ => {}
            }
        }
        fail_pending(&pending, "sync daemon terminated");
    }));
}

/// Decode one NDJSON line, routing responses to their waiter and notifications
/// to `emit`.
fn handle_line(emit: &impl Fn(&'static str, Value), pending: &Pending, line: &str) {
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
        Ok(RpcMessage::Notification(notification)) => {
            match notification_event(&notification.method) {
                Some(event) => emit(event, notification.params),
                None => tracing::debug!(
                    method = notification.method.as_str(),
                    "unhandled sync daemon notification"
                ),
            }
        }
        Ok(RpcMessage::Request(_)) => {
            tracing::debug!("ignoring inbound request from sync daemon");
        }
        Err(error) => {
            tracing::warn!(error = %error, "sync daemon emitted an unparsable line");
        }
    }
}

/// Map a daemon notification method onto its `sync://…` Tauri event.
fn notification_event(method: &str) -> Option<&'static str> {
    match method {
        rpc::method::NOTIFY_PAIRING_REQUESTED => Some("sync://pairing-requested"),
        rpc::method::NOTIFY_REQUEST_RECEIVED => Some("sync://request"),
        rpc::method::NOTIFY_SYNC_COMPLETED => Some("sync://completed"),
        rpc::method::NOTIFY_SERVER_STARTED => Some("sync://server-started"),
        rpc::method::NOTIFY_SERVER_STOPPED => Some("sync://server-stopped"),
        _ => None,
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

/// A [`SyncHandle`] wired to an in-memory duplex that is never exercised.
///
/// Integration tests link against the crate without `cfg(test)`, so
/// `test_fixture` cannot use [`testing::connect`]; this always-compiled
/// constructor lets `mock_state_for_bulk` fill the `sync` field.
pub(crate) fn fixture_handle() -> SyncHandle {
    let (client, _peer) = tokio::io::duplex(1024);
    let (_stdout, events) = tokio::sync::mpsc::channel(1);
    SyncHandle::connect(client, events, |_, _| {}, Duration::from_secs(1))
}

/// In-memory daemon doubles for driving a [`SyncHandle`] without a sidecar.
#[cfg(test)]
pub(crate) mod testing {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::{Value, json};
    use tauri_plugin_shell::process::{CommandEvent, TerminatedPayload};
    use tokio::io::{AsyncBufReadExt, BufReader, DuplexStream, Lines};
    use tokio::sync::mpsc;

    use super::SyncHandle;

    /// Notifications the handle re-emitted, in order.
    pub(crate) type Emitted = Arc<Mutex<Vec<(&'static str, Value)>>>;

    /// `(method, params)` of every request a [`scripted`] daemon received.
    pub(crate) type Recorded = Arc<Mutex<Vec<(String, Value)>>>;

    /// The daemon end of a [`SyncHandle`] wired over in-memory pipes.
    pub(crate) struct FakeDaemon {
        /// Request lines the handle wrote to the daemon's stdin.
        pub(crate) stdin: Lines<BufReader<DuplexStream>>,
        /// The daemon's stdout, delivered as the shell plugin would.
        pub(crate) stdout: mpsc::Sender<CommandEvent>,
        pub(crate) emitted: Emitted,
    }

    /// A handle whose calls time out after `timeout`, plus its daemon end.
    pub(crate) fn connect(timeout: Duration) -> (SyncHandle, FakeDaemon) {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (stdout, events) = mpsc::channel(16);
        let emitted = Emitted::default();
        let sink = Arc::clone(&emitted);
        let handle = SyncHandle::connect(
            client,
            events,
            move |event, payload| sink.lock().unwrap().push((event, payload)),
            timeout,
        );
        let daemon = FakeDaemon {
            stdin: BufReader::new(server).lines(),
            stdout,
            emitted,
        };
        (handle, daemon)
    }

    impl FakeDaemon {
        /// The next request the handle sent, decoded.
        pub(crate) async fn next_request(&mut self) -> Value {
            let line = self
                .stdin
                .next_line()
                .await
                .unwrap()
                .expect("the handle wrote a request line");
            serde_json::from_str(&line).unwrap()
        }

        pub(crate) async fn write_line(&self, line: &str) {
            self.stdout
                .send(CommandEvent::Stdout(line.as_bytes().to_vec()))
                .await
                .unwrap();
        }

        pub(crate) async fn respond(&self, id: &Value, result: Value) {
            let line = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            self.write_line(&line.to_string()).await;
        }

        pub(crate) async fn respond_error(&self, id: &Value, message: &str) {
            let line = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32603, "message": message },
            });
            self.write_line(&line.to_string()).await;
        }

        pub(crate) async fn terminate(&self) {
            self.stdout
                .send(CommandEvent::Terminated(TerminatedPayload {
                    code: Some(0),
                    signal: None,
                }))
                .await
                .unwrap();
        }
    }

    /// A daemon that answers every request with `answer(method)`: `Ok` as the
    /// result, `Err` as an RPC error carrying that message.
    pub(crate) fn scripted<F>(answer: F) -> (SyncHandle, Recorded)
    where
        F: Fn(&str) -> Result<Value, String> + Send + 'static,
    {
        let (handle, mut daemon) = connect(Duration::from_secs(10));
        let recorded = Recorded::default();
        let log = Arc::clone(&recorded);
        tokio::spawn(async move {
            while let Ok(Some(line)) = daemon.stdin.next_line().await {
                let request: Value = serde_json::from_str(&line).unwrap();
                let method = request["method"].as_str().unwrap().to_string();
                log.lock()
                    .unwrap()
                    .push((method.clone(), request["params"].clone()));
                let reply = answer(&method);
                match reply {
                    Ok(result) => daemon.respond(&request["id"], result).await,
                    Err(message) => daemon.respond_error(&request["id"], &message).await,
                }
            }
        });
        (handle, recorded)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    use livtet_sync_server::rpc::method;
    use serde_json::{Value, json};
    use tauri_plugin_shell::process::CommandEvent;
    use tokio::sync::oneshot;

    use super::testing::connect;
    use super::{Pending, SyncHandle, fail_pending};
    use crate::error::SyncError;

    /// Long enough that reaching it means a call was never resolved.
    const PATIENT: Duration = Duration::from_secs(60);

    fn pending_count(handle: &SyncHandle) -> usize {
        handle.pending.lock().unwrap().len()
    }

    #[test]
    fn test_fail_pending_drains_registry() {
        let (tx, mut rx) = oneshot::channel::<Result<serde_json::Value, String>>();
        let pending: Pending = Arc::new(Mutex::new([(1u64, tx)].into_iter().collect()));
        fail_pending(&pending, "test termination");
        assert!(pending.lock().unwrap().is_empty());
        assert_eq!(rx.try_recv().unwrap(), Err("test termination".to_string()));
    }

    #[tokio::test]
    async fn out_of_order_responses_resolve_their_own_callers() {
        let (handle, mut daemon) = connect(PATIENT);

        let (first, second, ()) = tokio::join!(
            handle.call("alpha", json!({ "n": 1 })),
            handle.call("beta", json!({ "n": 2 })),
            async {
                let a = daemon.next_request().await;
                let b = daemon.next_request().await;
                assert_ne!(a["id"], b["id"], "concurrent calls must get distinct ids");
                // Answer in reverse arrival order, echoing what each request carried.
                for request in [b, a] {
                    assert_eq!(request["jsonrpc"], "2.0");
                    let echo = json!({ "method": request["method"], "params": request["params"] });
                    daemon.respond(&request["id"], echo).await;
                }
            }
        );

        assert_eq!(
            first.unwrap(),
            json!({ "method": "alpha", "params": { "n": 1 } })
        );
        assert_eq!(
            second.unwrap(),
            json!({ "method": "beta", "params": { "n": 2 } })
        );
        assert_eq!(pending_count(&handle), 0);
    }

    #[tokio::test]
    async fn daemon_error_response_becomes_an_rpc_error() {
        let (handle, mut daemon) = connect(PATIENT);

        let (result, ()) = tokio::join!(handle.call(method::STATUS, Value::Null), async {
            let request = daemon.next_request().await;
            daemon
                .respond_error(&request["id"], "database locked")
                .await;
        });

        match result {
            Err(SyncError::Rpc { message }) => assert_eq!(message, "database locked"),
            other => panic!("expected an rpc error, got {other:?}"),
        }
        assert_eq!(pending_count(&handle), 0);
    }

    /// A response with neither `result` nor `error` must not read as success.
    #[tokio::test]
    async fn response_without_result_or_error_fails_the_call() {
        let (handle, mut daemon) = connect(PATIENT);

        let (result, ()) = tokio::join!(handle.call(method::HEALTH, Value::Null), async {
            let request = daemon.next_request().await;
            let bare = json!({ "jsonrpc": "2.0", "id": request["id"] });
            daemon.write_line(&bare.to_string()).await;
        });

        assert!(
            matches!(result, Err(SyncError::Rpc { .. })),
            "expected an rpc error, got {result:?}"
        );
    }

    /// Garbage, stray responses, inbound requests (even one reusing the pending
    /// id), stderr and stream errors are all skipped; the reader keeps going
    /// and still resolves the real response.
    #[tokio::test]
    async fn reader_survives_noise_and_resolves_the_real_response() {
        let (handle, mut daemon) = connect(PATIENT);

        let (result, ()) = tokio::join!(handle.call(method::HEALTH, Value::Null), async {
            let request = daemon.next_request().await;
            let id = request["id"].clone();
            let inbound_request =
                json!({ "jsonrpc": "2.0", "id": id, "method": "ping" }).to_string();
            for noise in [
                "not json at all",
                "{}",
                "   ",
                r#"{"jsonrpc":"2.0","id":"one","result":"non-numeric id"}"#,
                r#"{"jsonrpc":"2.0","id":999,"result":"nobody asked"}"#,
                inbound_request.as_str(),
                r#"{"jsonrpc":"2.0","method":"mystery.event","params":{}}"#,
            ] {
                daemon.write_line(noise).await;
            }
            daemon
                .stdout
                .send(CommandEvent::Stderr(b"warning: disk is slow".to_vec()))
                .await
                .unwrap();
            daemon
                .stdout
                .send(CommandEvent::Error("invalid utf-8".to_string()))
                .await
                .unwrap();
            daemon.respond(&id, json!("pong")).await;
        });

        assert_eq!(result.unwrap(), json!("pong"));
        assert!(daemon.emitted.lock().unwrap().is_empty());
    }

    /// Each known notification becomes its `sync://` event with the daemon's
    /// params, in order; unknown ones are dropped; none of them resolves the
    /// call that is waiting meanwhile.
    #[tokio::test]
    async fn notifications_are_routed_to_events_not_to_waiting_calls() {
        let (handle, mut daemon) = connect(PATIENT);
        let routes = [
            (method::NOTIFY_PAIRING_REQUESTED, "sync://pairing-requested"),
            (method::NOTIFY_REQUEST_RECEIVED, "sync://request"),
            (method::NOTIFY_SYNC_COMPLETED, "sync://completed"),
            (method::NOTIFY_SERVER_STARTED, "sync://server-started"),
            (method::NOTIFY_SERVER_STOPPED, "sync://server-stopped"),
        ];

        let (result, ()) = tokio::join!(handle.call(method::STATUS, Value::Null), async {
            let request = daemon.next_request().await;
            for (seq, (notification, _)) in routes.iter().enumerate() {
                let line =
                    json!({ "jsonrpc": "2.0", "method": notification, "params": { "seq": seq } });
                daemon.write_line(&line.to_string()).await;
            }
            daemon
                .write_line(r#"{"jsonrpc":"2.0","method":"unknown.event","params":{"seq":99}}"#)
                .await;
            daemon
                .respond(&request["id"], json!({ "done": true }))
                .await;
        });

        assert_eq!(result.unwrap(), json!({ "done": true }));
        let expected: Vec<(&str, Value)> = routes
            .iter()
            .enumerate()
            .map(|(seq, (_, event))| (*event, json!({ "seq": seq })))
            .collect();
        assert_eq!(*daemon.emitted.lock().unwrap(), expected);
    }

    #[tokio::test]
    async fn daemon_termination_fails_in_flight_calls_promptly() {
        let (handle, mut daemon) = connect(PATIENT);

        let (result, ()) = tokio::join!(handle.call(method::SERVER_START, Value::Null), async {
            daemon.next_request().await;
            daemon.terminate().await;
        });

        // Not the timeout's `Unavailable`: the waiter was released by the reader.
        assert!(
            matches!(result, Err(SyncError::Rpc { .. })),
            "expected an rpc error, got {result:?}"
        );
        assert_eq!(pending_count(&handle), 0);
    }

    /// The output stream closing without a `Terminated` event is just as final.
    #[tokio::test]
    async fn output_eof_fails_in_flight_calls_promptly() {
        let (handle, mut daemon) = connect(PATIENT);

        let (result, ()) =
            tokio::join!(handle.call(method::DEVICES_LIST, Value::Null), async move {
                daemon.next_request().await;
                drop(daemon);
            });

        assert!(
            matches!(result, Err(SyncError::Rpc { .. })),
            "expected an rpc error, got {result:?}"
        );
        assert_eq!(pending_count(&handle), 0);
    }

    #[tokio::test]
    async fn write_failure_is_unavailable_and_unparks_the_request() {
        let (handle, daemon) = connect(PATIENT);
        drop(daemon.stdin);

        let result = handle.call(method::HEALTH, Value::Null).await;

        assert!(
            matches!(result, Err(SyncError::Unavailable { .. })),
            "expected unavailable, got {result:?}"
        );
        assert_eq!(pending_count(&handle), 0);
    }

    #[tokio::test]
    async fn timed_out_call_is_unavailable_and_its_late_answer_reaches_no_one() {
        let (mut handle, mut daemon) = connect(Duration::from_millis(50));

        let (timed_out, abandoned) = tokio::join!(
            handle.call(method::STATUS, Value::Null),
            daemon.next_request()
        );

        assert!(
            matches!(timed_out, Err(SyncError::Unavailable { .. })),
            "expected unavailable, got {timed_out:?}"
        );
        assert_eq!(pending_count(&handle), 0);

        handle.timeout = PATIENT;
        let (next, ()) = tokio::join!(handle.call(method::HEALTH, Value::Null), async {
            let request = daemon.next_request().await;
            daemon.respond(&abandoned["id"], json!("stale")).await;
            daemon.respond(&request["id"], json!("fresh")).await;
        });

        assert_eq!(next.unwrap(), json!("fresh"));
    }

    #[tokio::test]
    async fn shutdown_sends_shutdown_and_returns_once_the_daemon_exits() {
        let (handle, mut daemon) = connect(PATIENT);

        let shutdown = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(handle.shutdown(), async {
                let request = daemon.next_request().await;
                daemon.terminate().await;
                request
            })
        })
        .await;

        let ((), request) = shutdown.expect("shutdown waited for the call timeout");
        assert_eq!(request["method"], method::SHUTDOWN);
        assert_eq!(request["params"], Value::Null);
    }
}
