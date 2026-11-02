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

use crate::sync_settings::SyncSettingsStore;

/// How often the daemon reads every paired peer, in seconds.
pub(crate) const PULL_INTERVAL_SECS: u64 = 5;

/// How long a JSON-RPC call may wait for its response, by default.
///
/// Control calls are answered from memory or one SQLite query, so ten seconds
/// is generous. The methods that talk to a peer are not in that class; they get
/// their own budget from [`timeout_for`].
const RPC_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `method` may take before the caller gives up.
///
/// A pull waits on the peer: an HTTP request, the offered file and cover
/// written to disk, then `apply_changes` over every row. An offer decision adds
/// the download itself. Sharing the ten-second control budget with them made a
/// working pull report "the sync daemon did not respond in time" while the
/// daemon was still applying it.
fn timeout_for(method: &str, default: Duration) -> Duration {
    match method {
        rpc::method::DEVICES_PULL => Duration::from_secs(300),
        rpc::method::OFFERS_COVER => Duration::from_secs(120),
        rpc::method::OFFERS_DECIDE => Duration::from_secs(600),
        _ => default,
    }
}

/// The daemon sidecar name, as declared in `bundle.externalBin`.
const SIDECAR: &str = "livtet-sync-daemon";

/// Ports tried when the remembered one is unavailable.
///
/// A single number would fight whatever else the machine runs on a given day,
/// and a range the kernel is unlikely to hand to anything else keeps the
/// chosen port stable in practice. Above the ephemeral band, so nothing here
/// competes with the daemon's own outbound connections.
const PORT_SCAN: std::ops::RangeInclusive<u16> = 7710..=7769;

/// How many `status` polls to spend waiting for the embedded server's port.
///
/// The daemon answers `status` before its server task is up, so the answer is
/// polled; a daemon that could not bind exits instead of serving, and its pipe
/// closes long before this budget runs out.
const PORT_SETTLE_ATTEMPTS: usize = 40;

/// Pause between `status` polls while settling the listen port.
const PORT_SETTLE_PAUSE: Duration = Duration::from_millis(50);

/// The `RUST_LOG` the sidecar is spawned with.
///
/// A child inherits this process's environment, so without this the daemon would
/// inherit a dev `RUST_LOG` whose trailing bare `trace` turns iroh's per-packet
/// logging into a firehose — and every line of it comes back here over stderr,
/// where [`start_reader`] re-emits it. Appending the same transport mutes the
/// desktop applies ([`crate::QUIET_TRANSPORTS`]) is what makes the sidecar quiet
/// at the source; it cannot be filtered on this side, since it arrives as text.
fn sidecar_log_filter() -> String {
    let mut filter = std::env::var("RUST_LOG").unwrap_or_else(|_| String::from("info"));
    for directive in crate::QUIET_TRANSPORTS {
        filter.push(',');
        filter.push_str(directive);
    }
    filter
}

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
    /// `listen_port` is what the embedded server binds; `None` lets the kernel
    /// pick. Prefer [`SyncHandle::spawn_with_remembered_port`], which resolves
    /// and records the port instead of leaving peers with a stale address.
    /// `lan` selects `--lan` (every interface) over the loopback default;
    /// `device_name` is advertised to nearby peers, and a blank or absent value
    /// makes the daemon generate one from the device id.
    pub fn spawn(
        app: &AppHandle,
        db_path: &Utf8Path,
        device_name: Option<&str>,
        lan: bool,
        books_dir: &Utf8Path,
        covers_dir: &Utf8Path,
        listen_port: Option<u16>,
    ) -> Result<Self, SyncError> {
        let args = sidecar_args(
            db_path,
            device_name,
            lan,
            books_dir,
            covers_dir,
            listen_port,
        );

        let command = app
            .shell()
            .sidecar(SIDECAR)
            .map_err(SyncError::unavailable)?
            .args(args)
            .env("RUST_LOG", sidecar_log_filter());

        let (rx, child) = command.spawn().map_err(SyncError::unavailable)?;

        let app = app.clone();
        let emit = move |event: &'static str, payload: Value| {
            if let Err(error) = app.emit(event, payload) {
                tracing::warn!(error = %error, event, "failed to emit sync daemon event");
            }
        };

        Ok(Self::connect(ChildStdin(child), rx, emit, RPC_TIMEOUT))
    }

    /// Spawn the daemon on a port that survives a restart, and remember it.
    ///
    /// A paired peer stores this device's address once, when the pairing is
    /// approved, so an ephemeral port strands every existing pairing the moment
    /// the app restarts: the peer keeps dialling a port nothing listens on. The
    /// remembered port is therefore asked for again, and whatever the daemon
    /// ends up binding is written back so the next launch can ask for that.
    ///
    /// A daemon that cannot bind the port — another process grabbed it between
    /// the probe and the bind — exits before serving, so the spawn is retried
    /// once with an ephemeral port and the kernel's choice is what gets
    /// remembered. Either way the returned handle is the one that is running.
    pub async fn spawn_with_remembered_port(
        app: &AppHandle,
        settings: &dyn SyncSettingsStore,
        db_path: &Utf8Path,
        device_name: Option<&str>,
        lan: bool,
        books_dir: &Utf8Path,
        covers_dir: &Utf8Path,
    ) -> Result<Self, SyncError> {
        let remembered = settings.read().and_then(|settings| settings.listen_port);
        let wanted = choose_listen_port(bind_host(lan), remembered);

        let mut handle = Self::spawn(
            app,
            db_path,
            device_name,
            lan,
            books_dir,
            covers_dir,
            wanted,
        )?;
        let mut bound = handle.served_port().await;
        if bound.is_none() {
            tracing::warn!(
                wanted = wanted.map(i32::from),
                "sync daemon did not serve the requested port; retrying on an ephemeral one"
            );
            handle.shutdown().await;
            handle = Self::spawn(app, db_path, device_name, lan, books_dir, covers_dir, None)?;
            bound = handle.served_port().await;
        }

        if let Some(port) = bound
            && remembered != Some(port)
        {
            let next = settings
                .read()
                .unwrap_or_default()
                .with_listen_port(Some(port));
            if let Err(error) = settings.write(&next) {
                // The daemon is up and reachable; only the next launch loses out.
                tracing::warn!(%error, port, "could not remember the sync port");
            }
        }
        Ok(handle)
    }

    /// The port the embedded server is serving on, waiting briefly for it to
    /// come up.
    async fn served_port(&self) -> Option<u16> {
        for _ in 0..PORT_SETTLE_ATTEMPTS {
            if let Ok(status) = self.call(rpc::method::STATUS, Value::Null).await {
                let running = status
                    .get("server_running")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let port = status
                    .get("port")
                    .and_then(Value::as_u64)
                    .and_then(|port| u16::try_from(port).ok())
                    .filter(|port| *port > 0);
                if running && port.is_some() {
                    return port;
                }
            }
            tokio::time::sleep(PORT_SETTLE_PAUSE).await;
        }
        None
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

        match tokio::time::timeout(timeout_for(method, self.timeout), rx).await {
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

/// The daemon arguments for a spawn.
///
/// Split out from [`SyncHandle::spawn`] so the flag selection is testable
/// without a Tauri app: `--lan` replaces the loopback host, never accompanies
/// it.
fn sidecar_args(
    db_path: &Utf8Path,
    device_name: Option<&str>,
    lan: bool,
    books_dir: &Utf8Path,
    covers_dir: &Utf8Path,
    listen_port: Option<u16>,
) -> Vec<String> {
    let mut args = vec![
        "--db".to_string(),
        db_path.as_str().to_string(),
        "--port".to_string(),
        listen_port.map_or_else(|| "0".to_string(), |port| port.to_string()),
        "--books-dir".to_string(),
        books_dir.to_string(),
        "--covers-dir".to_string(),
        covers_dir.to_string(),
        // The peer cannot push, so this is the only thing that makes a book
        // arrive without the reader asking. Five seconds costs one dial and
        // one query per paired peer; a one-second loop would spend that
        // continuously for a change that is not urgent.
        "--pull-interval-secs".to_string(),
        PULL_INTERVAL_SECS.to_string(),
    ];
    if lan {
        args.push("--lan".to_string());
    } else {
        args.push("--host".to_string());
        args.push("127.0.0.1".to_string());
    }
    let chosen_name = device_name.map(str::trim).filter(|name| !name.is_empty());
    if let Some(name) = chosen_name {
        args.push("--device-name".to_string());
        args.push(name.to_string());
    }
    args
}

/// The interface the embedded server binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindHost {
    /// `--lan`: every interface, which is what makes a peer on the network
    /// able to reach this device at all.
    EveryInterface,
    /// `--host 127.0.0.1`: this machine only.
    Loopback,
}

impl BindHost {
    fn socket_addr(self, port: u16) -> std::net::SocketAddr {
        match self {
            Self::EveryInterface => std::net::SocketAddr::from(([0, 0, 0, 0], port)),
            Self::Loopback => std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        }
    }
}

/// The interface the daemon will bind for this `lan` setting.
fn bind_host(lan: bool) -> BindHost {
    if lan {
        BindHost::EveryInterface
    } else {
        BindHost::Loopback
    }
}

/// Whether the daemon would be able to bind `port`.
///
/// Binding is the only honest probe: it answers for every holder of the port,
/// not just for other Livtet processes. The listener is dropped immediately, so
/// the daemon's own bind follows a moment later.
fn port_is_free(host: BindHost, port: u16) -> bool {
    std::net::TcpListener::bind(host.socket_addr(port)).is_ok()
}

/// The port to ask the daemon for: the remembered one when it is still free,
/// otherwise the first free port in [`PORT_SCAN`], otherwise `None` so the
/// kernel picks and [`SyncHandle::spawn_with_remembered_port`] records that.
fn choose_listen_port(host: BindHost, remembered: Option<u16>) -> Option<u16> {
    if let Some(port) = remembered.filter(|port| port_is_free(host, *port)) {
        return Some(port);
    }
    PORT_SCAN.clone().find(|port| port_is_free(host, *port))
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
                        forward_daemon_line(line);
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

/// Re-emit one line of the daemon's stderr at the level it was written at.
///
/// The daemon writes `RUST_LOG` output itself, so each line already carries its
/// level: `2026-11-01T13:08:46.463650Z  INFO livtet_sync_daemon: …`. Forwarding
/// everything at WARN — which is what this did — made a startup notice and a
/// sqlx DEBUG trace indistinguishable from a real warning, and put them in the
/// log at a level nothing filters them out at.
fn forward_daemon_line(line: &str) {
    match daemon_level(line) {
        Some("ERROR") => tracing::error!(target: "livtet_sync_daemon", "{line}"),
        Some("WARN") => tracing::warn!(target: "livtet_sync_daemon", "{line}"),
        Some("INFO") => tracing::info!(target: "livtet_sync_daemon", "{line}"),
        Some("DEBUG") => tracing::debug!(target: "livtet_sync_daemon", "{line}"),
        Some("TRACE") => tracing::trace!(target: "livtet_sync_daemon", "{line}"),
        // No level at all is the daemon failing before its subscriber exists:
        // an `eprintln!` from argument parsing, or a panic message.
        _ => tracing::warn!(target: "livtet_sync_daemon", "{line}"),
    }
}

/// The level token a formatted daemon line carries, or `None` when it has none.
fn daemon_level(line: &str) -> Option<&str> {
    let level = line.split_whitespace().nth(1)?;
    matches!(level, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR").then_some(level)
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
        rpc::method::NOTIFY_PAIRING_CLAIMED => Some("sync://paired"),
        rpc::method::NOTIFY_REQUEST_RECEIVED => Some("sync://request"),
        rpc::method::NOTIFY_SYNC_COMPLETED => Some("sync://completed"),
        rpc::method::NOTIFY_SYNC_PULLED => Some("sync://pulled"),
        rpc::method::NOTIFY_OFFER_RECEIVED => Some("sync://offer-received"),
        rpc::method::NOTIFY_OFFER_DECIDED => Some("sync://offer-decided"),
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

    use super::timeout_for;
    use livtet_sync_server::rpc::method;
    use serde_json::{Value, json};
    use tauri_plugin_shell::process::CommandEvent;
    use tokio::sync::oneshot;

    use super::testing::connect;
    use super::{
        BindHost, Pending, SyncHandle, choose_listen_port, daemon_level, fail_pending,
        port_is_free, sidecar_args,
    };
    use crate::error::SyncError;

    /// A pull is bounded by the peer, not by the daemon's own speed: sharing
    /// the ten-second control budget made a working pull look like a dead
    /// daemon while it was still applying rows.
    #[test]
    fn peer_facing_methods_get_a_longer_budget_than_control_calls() {
        let default = Duration::from_secs(10);

        assert_eq!(timeout_for(method::HEALTH, default), default);
        assert_eq!(timeout_for(method::DEVICES_LIST, default), default);
        assert!(timeout_for(method::DEVICES_PULL, default) >= Duration::from_secs(300));
        assert!(
            timeout_for(method::OFFERS_DECIDE, default)
                > timeout_for(method::DEVICES_PULL, default),
            "a decision downloads the file, so it needs more room than a pull"
        );
    }

    /// Long enough that reaching it means a call was never resolved.
    const PATIENT: Duration = Duration::from_secs(60);

    /// The daemon formats its own lines, so the level is the token after the
    /// timestamp. Bare text is what the daemon prints before its subscriber
    /// exists — a bad argument, a panic — and must stay a warning.
    #[test]
    fn daemon_lines_keep_the_level_they_were_written_at() {
        assert_eq!(
            daemon_level("2026-11-01T13:08:46.463650Z  INFO livtet_sync_daemon: starting"),
            Some("INFO"),
            "the default format pads the level, it does not change it"
        );
        assert_eq!(
            daemon_level("2026-11-01T13:08:46.463650Z DEBUG sqlx::query: summary=\"PRAGMA\""),
            Some("DEBUG")
        );
        assert_eq!(
            daemon_level("2026-11-01T13:08:46.463650Z  WARN livtet_sync_daemon: retrying"),
            Some("WARN")
        );
        assert_eq!(
            daemon_level("livtet-sync-daemon: --port needs a number"),
            None
        );
        assert_eq!(
            daemon_level("thread 'main' panicked at src/main.rs:1:1"),
            None
        );
    }

    fn pending_count(handle: &SyncHandle) -> usize {
        handle.pending.lock().unwrap().len()
    }

    fn args(db: &str, name: Option<&str>, lan: bool) -> Vec<String> {
        sidecar_args(
            camino::Utf8Path::new(db),
            name,
            lan,
            camino::Utf8Path::new("/tmp/books"),
            camino::Utf8Path::new("/tmp/covers"),
            None,
        )
    }

    #[test]
    fn lan_replaces_the_loopback_host() {
        let args = args("/tmp/livtet.db", None, true);
        assert!(args.contains(&"--lan".to_string()));
        assert!(
            !args.contains(&"--host".to_string()),
            "the two bind flags must not be sent together: {args:?}"
        );
    }

    #[test]
    fn loopback_is_the_fall_back() {
        let args = args("/tmp/livtet.db", None, false);
        assert!(!args.contains(&"--lan".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--host", "127.0.0.1"]));
    }

    #[test]
    fn a_chosen_name_is_trimmed_and_blank_is_omitted() {
        let named = args("/tmp/livtet.db", Some("  Nimble Quail  "), true);
        assert!(
            named
                .windows(2)
                .any(|pair| pair == ["--device-name", "Nimble Quail"])
        );

        let blank = args("/tmp/livtet.db", Some("   "), true);
        assert!(!blank.contains(&"--device-name".to_string()));
    }

    #[test]
    fn the_remembered_port_is_what_the_daemon_is_asked_for() {
        let ported = sidecar_args(
            camino::Utf8Path::new("/tmp/livtet.db"),
            None,
            true,
            camino::Utf8Path::new("/tmp/books"),
            camino::Utf8Path::new("/tmp/covers"),
            Some(7714),
        );
        assert!(
            ported.windows(2).any(|pair| pair == ["--port", "7714"]),
            "a remembered port must reach the daemon: {ported:?}"
        );

        let ephemeral = args("/tmp/livtet.db", None, true);
        assert!(
            ephemeral.windows(2).any(|pair| pair == ["--port", "0"]),
            "without a remembered port the kernel picks: {ephemeral:?}"
        );
    }

    /// A remembered port another process already holds cannot be bound, so the
    /// daemon would exit on startup and the app would lose sync entirely.
    #[test]
    fn a_taken_port_is_skipped_for_one_that_can_be_bound() {
        let taken = std::net::TcpListener::bind(std::net::SocketAddr::from(([0, 0, 0, 0], 0)))
            .expect("ephemeral listener");
        let taken_port = taken.local_addr().expect("listener address").port();

        let chosen = choose_listen_port(BindHost::EveryInterface, Some(taken_port));

        assert_ne!(chosen, Some(taken_port), "a held port must not be reused");
        let chosen = chosen.expect("a free port inside the scan range");
        assert!(
            port_is_free(BindHost::EveryInterface, chosen),
            "{chosen} must be bindable, or the daemon exits instead of serving"
        );
    }

    #[test]
    fn a_free_remembered_port_is_preferred_over_the_scan() {
        let free = std::net::TcpListener::bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
            .expect("ephemeral listener");
        let free_port = free.local_addr().expect("listener address").port();
        drop(free);

        assert_eq!(
            choose_listen_port(BindHost::Loopback, Some(free_port)),
            Some(free_port),
            "stability is the whole point: the remembered port must win"
        );
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
            (method::NOTIFY_PAIRING_CLAIMED, "sync://paired"),
            (method::NOTIFY_REQUEST_RECEIVED, "sync://request"),
            (method::NOTIFY_SYNC_COMPLETED, "sync://completed"),
            (method::NOTIFY_SYNC_PULLED, "sync://pulled"),
            (method::NOTIFY_OFFER_RECEIVED, "sync://offer-received"),
            (method::NOTIFY_OFFER_DECIDED, "sync://offer-decided"),
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
