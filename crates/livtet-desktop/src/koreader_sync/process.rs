//! Child-process hosting for the KOReader sync server.
//!
//! The desktop re-executes its own binary with [`KOREADER_DAEMON_FLAG`]. The
//! child speaks no control protocol: it receives one [`DaemonConfig`] line on
//! stdin, answers with one [`DaemonReady`] line on stdout, and serves until
//! stdin reaches EOF. This module owns the parent side and the child entry
//! point; the routes and database work live in the parent module.

use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;

use camino::Utf8PathBuf;
use poem::listener::Listener;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;

use super::{
    KOREADER_DAEMON_FLAG, KoreaderRunner, KoreaderSyncDeps, KoreaderSyncError, Listening,
    SWEEP_BATCH, StoredKoreaderSync, SyncServerState, base_url, desired_address, ensure_tables,
    map_documents, route,
};

/// How long the parent waits for the child to bind.
const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything the child process needs, written as one JSON line on stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DaemonConfig {
    pub db_path: String,
    pub books_dir: String,
    pub settings: StoredKoreaderSync,
}

/// The child's readiness report on stdout: one line, then silence.
#[derive(Debug, Serialize, Deserialize)]
struct DaemonReady {
    ok: bool,
    url: Option<String>,
    code: Option<String>,
    message: Option<String>,
}

impl DaemonReady {
    fn failure(error: &KoreaderSyncError) -> Self {
        Self {
            ok: false,
            url: None,
            code: Some(error.code.clone()),
            message: Some(error.message.clone()),
        }
    }
}

/// A child process running the same executable in daemon mode.
struct ChildSession {
    child: Child,
    /// Kept open for the child's lifetime; closing it is the graceful stop.
    stdin: ChildStdin,
    listening: Listening,
    settings: StoredKoreaderSync,
}

/// Hosts the server in a separate process, as `livtet-sync-daemon` does.
#[derive(Default)]
pub(crate) struct ProcessRunner {
    current: Mutex<Option<ChildSession>>,
}

#[async_trait::async_trait]
impl KoreaderRunner for ProcessRunner {
    async fn reconcile(
        &self,
        settings: &StoredKoreaderSync,
        deps: &KoreaderSyncDeps,
    ) -> Result<Option<Listening>, KoreaderSyncError> {
        if deps.db_path.is_empty() {
            return Err(KoreaderSyncError::new(
                "daemon",
                "the KOReader daemon needs a database path",
            ));
        }

        let desired = desired_address(settings);
        let mut current = self.current.lock().await;
        // Saving an unchanged configuration must not bounce a working server.
        if let Some(session) = current.as_mut()
            && session.settings == *settings
            && matches!(session.child.try_wait(), Ok(None))
        {
            return Ok(Some(session.listening.clone()));
        }
        let previous = current.take();
        let previous_settings = previous.as_ref().map(|session| session.settings.clone());
        if let Some(previous) = previous {
            stop_child(previous).await;
        }

        if desired.is_none() {
            return Ok(None);
        }

        let config = DaemonConfig {
            db_path: deps.db_path.clone(),
            books_dir: deps.books_dir.to_string(),
            settings: settings.clone(),
        };
        match spawn_child(config).await {
            Ok(session) => {
                let listening = session.listening.clone();
                *current = Some(session);
                Ok(Some(listening))
            }
            Err(error) => {
                // The old listener was stopped before the replacement could
                // bind; put it back so a refused save is not an outage.
                if let Some(previous_settings) = previous_settings {
                    let restore = DaemonConfig {
                        db_path: deps.db_path.clone(),
                        books_dir: deps.books_dir.to_string(),
                        settings: previous_settings,
                    };
                    if let Ok(session) = spawn_child(restore).await {
                        *current = Some(session);
                    }
                }
                Err(error)
            }
        }
    }

    async fn stop(&self) -> Result<(), KoreaderSyncError> {
        let session = {
            let mut current = self.current.lock().await;
            current.take()
        };
        if let Some(session) = session {
            stop_child(session).await;
        }
        Ok(())
    }

    async fn listening(&self) -> Option<Listening> {
        let mut current = self.current.lock().await;
        let exited = current
            .as_mut()
            .map(|session| matches!(session.child.try_wait(), Ok(Some(_))))
            .unwrap_or(false);
        if exited {
            *current = None;
            return None;
        }
        current.as_ref().map(|session| session.listening.clone())
    }
}

/// Spawn the current executable with [`KOREADER_DAEMON_FLAG`], hand it the
/// config on stdin, and wait for the one-line readiness report.
async fn spawn_child(config: DaemonConfig) -> Result<ChildSession, KoreaderSyncError> {
    let Some((_host, port)) = desired_address(&config.settings) else {
        return Err(KoreaderSyncError::new(
            "daemon",
            "refusing to spawn a stopped KOReader daemon",
        ));
    };
    let exe = std::env::current_exe()
        .map_err(|error| KoreaderSyncError::new("daemon", error.to_string()))?;
    let mut command = tokio::process::Command::new(exe);
    command
        .arg(KOREADER_DAEMON_FLAG)
        // The GUI's RUST_LOG ends in a bare `trace` in dev; the child cannot be
        // filtered after its stderr reaches the parent, so keep it quiet here.
        .env("RUST_LOG", "livtet_desktop_lib=info,info")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command
        .spawn()
        .map_err(|error| KoreaderSyncError::new("daemon", error.to_string()))?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.start_kill();
        return Err(KoreaderSyncError::new("daemon", "no stdin pipe"));
    };
    let Some(stdout) = child.stdout.take() else {
        let _ = child.start_kill();
        return Err(KoreaderSyncError::new("daemon", "no stdout pipe"));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.start_kill();
        return Err(KoreaderSyncError::new("daemon", "no stderr pipe"));
    };

    let line = serde_json::to_string(&config)
        .map_err(|error| KoreaderSyncError::new("daemon", error.to_string()))?;
    let write = async {
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    }
    .await;
    if let Err(error) = write {
        let _ = child.start_kill();
        return Err(KoreaderSyncError::new("daemon", error.to_string()));
    }

    let mut lines = BufReader::new(stdout).lines();
    let ready = tokio::time::timeout(DAEMON_READY_TIMEOUT, lines.next_line()).await;
    let ready = match ready {
        Ok(Ok(Some(line))) => line,
        Ok(Ok(None)) => {
            let _ = child.start_kill();
            return Err(KoreaderSyncError::new(
                "daemon",
                "KOReader daemon exited before reporting readiness",
            ));
        }
        Ok(Err(error)) => {
            let _ = child.start_kill();
            return Err(KoreaderSyncError::new("daemon", error.to_string()));
        }
        Err(_) => {
            let _ = child.start_kill();
            return Err(KoreaderSyncError::new(
                "daemon",
                "timed out waiting for the KOReader daemon",
            ));
        }
    };
    let ready: DaemonReady = serde_json::from_str(&ready)
        .map_err(|error| KoreaderSyncError::new("daemon", error.to_string()))?;
    if !ready.ok {
        let _ = child.start_kill();
        return Err(KoreaderSyncError::new(
            ready.code.unwrap_or_else(|| "daemon".to_string()),
            ready
                .message
                .unwrap_or_else(|| "KOReader daemon failed to start".to_string()),
        ));
    }

    // Keep the pipes drained; stdout carries nothing after readiness.
    tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::warn!("koreader-daemon: {line}");
        }
    });

    let listening = Listening {
        url: ready
            .url
            .unwrap_or_else(|| base_url(&config.settings, port)),
    };
    Ok(ChildSession {
        child,
        stdin,
        listening,
        settings: config.settings,
    })
}

/// Close the config pipe and kill the child, then reap it.
async fn stop_child(session: ChildSession) {
    let ChildSession {
        mut child, stdin, ..
    } = session;
    drop(stdin);
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
}

/// Run the KOReader server in this process. The desktop binary calls this from
/// `main` when it sees [`KOREADER_DAEMON_FLAG`]; it never initializes Tauri.
pub fn run_koreader_daemon() -> i32 {
    init_daemon_tracing();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("livtet-koreader-daemon: {error}");
            return 2;
        }
    };

    runtime.block_on(async {
        let mut stdin = BufReader::new(tokio::io::stdin());
        let mut config_line = String::new();
        if let Err(error) = stdin.read_line(&mut config_line).await {
            eprintln!("livtet-koreader-daemon: {error}");
            return 2;
        }
        if config_line.trim().is_empty() {
            return 2;
        }
        let config: DaemonConfig = match serde_json::from_str(config_line.trim()) {
            Ok(config) => config,
            Err(error) => {
                print_ready(&DaemonReady::failure(&KoreaderSyncError::new(
                    "config",
                    error.to_string(),
                )));
                return 2;
            }
        };

        match daemon_serve(config, stdin).await {
            Ok(()) => 0,
            Err(error) => {
                print_ready(&DaemonReady::failure(&error));
                1
            }
        }
    })
}

/// Bind the configured address, report readiness, map the library, and serve
/// until the parent closes stdin.
async fn daemon_serve(
    config: DaemonConfig,
    mut stdin: BufReader<tokio::io::Stdin>,
) -> Result<(), KoreaderSyncError> {
    let db = livtet_core::data::SharedState::connect(&config.db_path, &[])
        .await
        .map_err(|error| KoreaderSyncError::new("database", error.to_string()))?;
    let connection = db.db_conn();
    ensure_tables(&connection).await?;

    let deps = KoreaderSyncDeps {
        db: connection,
        db_path: config.db_path,
        books_dir: Utf8PathBuf::from(&config.books_dir),
    };

    let Some((host, port)) = desired_address(&config.settings) else {
        print_ready(&DaemonReady {
            ok: true,
            url: None,
            code: None,
            message: None,
        });
        wait_for_stdin_eof(&mut stdin).await;
        return Ok(());
    };

    let address = SocketAddr::new(host, port);
    let acceptor = poem::listener::TcpListener::bind(address)
        .into_acceptor()
        .await
        .map_err(|error| KoreaderSyncError::new("bind", format!("bind {address}: {error}")))?;

    let state = SyncServerState {
        db: deps.db.clone(),
        books_dir: deps.books_dir.clone(),
        credentials: config.settings.credentials(),
    };
    let (shutdown, wait) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let result = poem::Server::new_with_acceptor(acceptor)
            .name("livtet-koreader")
            .run_with_graceful_shutdown(
                route(state),
                async {
                    let _ = wait.await;
                },
                Some(Duration::from_secs(5)),
            )
            .await;
        if let Err(error) = result {
            tracing::warn!(%error, "KOReader sync server task exited with an error");
        }
    });

    let url = base_url(&config.settings, port);
    print_ready(&DaemonReady {
        ok: true,
        url: Some(url),
        code: None,
        message: None,
    });

    let mapping = {
        let db = deps.db.clone();
        let books_dir = deps.books_dir.clone();
        tokio::spawn(async move {
            loop {
                match map_documents(&db, &books_dir, SWEEP_BATCH).await {
                    Ok(0) => break,
                    Ok(_) => tokio::task::yield_now().await,
                    Err(error) => {
                        tracing::warn!(
                            message = %error.message,
                            "KOReader document mapping stopped"
                        );
                        break;
                    }
                }
            }
        })
    };

    // The parent holds the write end for the child's lifetime. EOF (parent
    // exited or killed) is the shutdown signal, matching the sync daemon. If
    // the server task dies first, exit too: a process with no listener must not
    // keep claiming to be running.
    let mut server = server;
    tokio::select! {
        _ = &mut server => {
            mapping.abort();
            return Err(KoreaderSyncError::new("server", "KOReader server stopped"));
        }
        _ = wait_for_stdin_eof(&mut stdin) => {}
    }
    let _ = shutdown.send(());
    let _ = server.await;
    mapping.abort();
    Ok(())
}

fn print_ready(ready: &DaemonReady) {
    if let Ok(line) = serde_json::to_string(ready) {
        println!("{line}");
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
    }
}

fn init_daemon_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
}

async fn wait_for_stdin_eof(stdin: &mut BufReader<tokio::io::Stdin>) {
    let mut buffer = [0u8; 256];
    loop {
        match stdin.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}
