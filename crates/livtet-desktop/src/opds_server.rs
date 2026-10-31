//! OPDS 2.0 server that publishes the on-disk library to reading devices
//! (ADR-0045).
//!
//! The listener is `livtet-opds-server`'s [`PoemOpdsServer`] wired to the live
//! [`AppState`]: the same SQLite pool and the same Tantivy index the rest of the
//! app writes, so a book imported a moment ago is already in the feed. Settings
//! (on/off and port) persist in `opds-server.json` and are owned by
//! [`OpdsServerRuntime`], which `commands::opds_server` drives over IPC.
//!
//! Unlike the MCP endpoint ([`crate::mcp`], loopback only), this one binds every
//! interface: an e-reader on the LAN is the client, and a loopback-bound OPDS
//! feed would have nothing to serve. There is no authentication, so anyone who
//! can reach the port can read the library — see ADR-0045.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use async_trait::async_trait;
use livtet_core::data::SharedState;
use livtet_core::search::SearchIndex;
use livtet_opds_server::{
    LivtetOpdsRepository, OpdsHandler, OpdsServer, OpdsService, PoemOpdsServer, SharedOpdsHandler,
    SharedOpdsRepository,
};
use livtet_opds_types::{Feed, OpdsError, Publication};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex;

use crate::types::{AppState, ArcMut};

/// Port the server binds when the user has not chosen one.
pub(crate) const DEFAULT_PORT: u16 = 8080;

/// Key the settings are stored under inside `opds-server.json`.
pub(crate) const SETTINGS_KEY: &str = "opds-server";

/// How the server is configured, persisted under [`SETTINGS_KEY`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct OpdsServerSettings {
    /// Whether the server listens at all.
    pub enabled: bool,
    /// Port to bind on every interface.
    pub port: u16,
}

impl Default for OpdsServerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
        }
    }
}

/// What Settings shows: the stored configuration plus what is actually bound.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsServerStatus {
    /// The persisted configuration.
    pub settings: OpdsServerSettings,
    /// Whether a listener is bound right now.
    pub running: bool,
    /// The feed URL an e-reader is configured with, when a listener is bound.
    pub url: Option<String>,
}

/// A settings failure, shaped like the other command errors (`{ code, message }`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsServerError {
    /// Stable machine-readable reason: `invalid-port`, `storage`, `index`,
    /// `bind`, `stop`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl OpdsServerError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Where the settings live. The trait exists so tests and the `AppState`
/// fixture can supply an in-memory store instead of a Tauri store file.
pub(crate) trait OpdsServerSettingsStore: Send + Sync {
    /// The persisted settings, absent until the first save.
    fn read(&self) -> Option<OpdsServerSettings>;
    /// Replace the persisted settings and flush them.
    fn write(&self, settings: &OpdsServerSettings) -> Result<(), String>;
}

impl OpdsServerSettingsStore for tauri_plugin_store::Store<tauri::Wry> {
    fn read(&self) -> Option<OpdsServerSettings> {
        self.get(SETTINGS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    fn write(&self, settings: &OpdsServerSettings) -> Result<(), String> {
        let value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
        self.set(SETTINGS_KEY, value);
        self.save().map_err(|error| error.to_string())
    }
}

/// A bound listener: the server plus the URL its feed links are built from.
struct Running {
    server: PoemOpdsServer,
    url: String,
}

/// The live listener plus the settings that drive it.
///
/// Held on [`AppState`] so Settings can start, stop, or move the listener
/// without restarting the app.
pub struct OpdsServerRuntime {
    store: Arc<dyn OpdsServerSettingsStore>,
    running: Mutex<Option<Running>>,
}

impl OpdsServerRuntime {
    /// Build the runtime over a settings store; nothing is bound until
    /// [`OpdsServerRuntime::apply`] runs.
    pub(crate) fn new(store: Arc<dyn OpdsServerSettingsStore>) -> Self {
        Self {
            store,
            running: Mutex::new(None),
        }
    }

    /// The persisted settings, or the off-by-default baseline.
    pub(crate) fn settings(&self) -> OpdsServerSettings {
        self.store.read().unwrap_or_default()
    }

    /// The stored configuration plus what is actually bound.
    pub(crate) async fn status(&self) -> OpdsServerStatus {
        let running = self.running.lock().await;
        OpdsServerStatus {
            settings: self.settings(),
            running: running.is_some(),
            url: running.as_ref().map(|running| running.url.clone()),
        }
    }

    /// Reconcile the listener with `settings`, then persist them.
    ///
    /// A port of 0 is refused: it would bind an ephemeral port no e-reader
    /// could be configured with. A refused save changes nothing — neither the
    /// bound listener nor the stored settings.
    pub(crate) async fn save(
        &self,
        settings: OpdsServerSettings,
        deps: &OpdsServerDeps,
    ) -> Result<OpdsServerStatus, OpdsServerError> {
        if settings.port == 0 {
            return Err(OpdsServerError::new(
                "invalid-port",
                "port must be between 1 and 65535",
            ));
        }

        // Reach the requested configuration *before* remembering it. A port that
        // cannot bind (already taken) must not cost the listener that is serving
        // right now, and must not be written to `opds-server.json` — otherwise the
        // next launch starts with a configuration that cannot start.
        self.apply(&settings, deps).await?;
        self.store
            .write(&settings)
            .map_err(|message| OpdsServerError::new("storage", message))?;
        Ok(self.status().await)
    }

    /// Turn the server on at the stored port, and remember that choice.
    pub(crate) async fn start(
        &self,
        deps: &OpdsServerDeps,
    ) -> Result<OpdsServerStatus, OpdsServerError> {
        let settings = OpdsServerSettings {
            enabled: true,
            ..self.settings()
        };
        self.save(settings, deps).await
    }

    /// Turn the server off, and remember that choice.
    pub(crate) async fn stop(
        &self,
        deps: &OpdsServerDeps,
    ) -> Result<OpdsServerStatus, OpdsServerError> {
        let settings = OpdsServerSettings {
            enabled: false,
            ..self.settings()
        };
        self.save(settings, deps).await
    }

    /// Reconcile the listener with `settings`, leaving it alone when it already
    /// serves the requested port.
    async fn apply(
        &self,
        settings: &OpdsServerSettings,
        deps: &OpdsServerDeps,
    ) -> Result<(), OpdsServerError> {
        let desired = desired_port(settings);

        let mut running = self.running.lock().await;
        if let (Some(port), Some(current)) = (desired, running.as_ref())
            && current.server.bound_addr().map(|addr| addr.port()) == Some(port)
        {
            return Ok(());
        }

        let Some(port) = desired else {
            if let Some(mut current) = running.take() {
                current
                    .server
                    .stop()
                    .await
                    .map_err(|error| OpdsServerError::new("stop", error.to_string()))?;
                tracing::info!("OPDS server stopped");
            }
            return Ok(());
        };

        // Bind the replacement first: a failed bind then leaves the listener that
        // is already serving untouched.
        let url = base_url(port);
        let mut server = PoemOpdsServer::new(deps.handler(url.clone()).await?);
        server
            .start(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port))
            .await
            .map_err(|error| OpdsServerError::new("bind", error.to_string()))?;
        tracing::info!(%url, "OPDS server listening on every interface");

        if let Some(mut previous) = running.replace(Running { server, url })
            && let Err(error) = previous.server.stop().await
        {
            // The new listener is live and carries the requested configuration;
            // a join failure only means the old task did not shut down cleanly.
            tracing::warn!(%error, "the previous OPDS listener did not shut down cleanly");
        }
        Ok(())
    }
}

/// Start the server from persisted settings.
///
/// Called once during app setup. A failure is logged, not fatal: the app runs
/// without the server either way.
pub(crate) async fn start_from_settings(runtime: &OpdsServerRuntime, deps: &OpdsServerDeps) {
    let settings = runtime.settings();
    if let Err(error) = runtime.apply(&settings, deps).await {
        tracing::warn!(
            code = %error.code,
            message = %error.message,
            "OPDS server did not start"
        );
    }
}

/// The port to bind, or `None` when the server should be off.
///
/// A stored port of 0 is treated as off: [`OpdsServerRuntime::save`] refuses to
/// write one, but a hand-edited store file must not bind a random port.
fn desired_port(settings: &OpdsServerSettings) -> Option<u16> {
    (settings.enabled && settings.port != 0).then_some(settings.port)
}

/// The feed URL the server publishes.
///
/// Links inside the feed are absolute, so they must name an address an
/// e-reader can reach: the machine's LAN address when it has one, loopback
/// otherwise (a single-machine setup, where the browser and the app share an
/// address space).
fn base_url(port: u16) -> String {
    match lan_ipv4() {
        Some(ip) => format!("http://{ip}:{port}"),
        None => format!("http://{}:{port}", Ipv4Addr::LOCALHOST),
    }
}

/// The machine's primary IPv4 address.
///
/// A UDP `connect` sends no packet: it asks the kernel which local address the
/// route to `192.0.2.1` (TEST-NET-1, never routed) would use, which is the
/// address an e-reader on the same network would reach this machine at.
fn lan_ipv4() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// The parts of [`AppState`] the server reads: the database and the index.
///
/// It deliberately excludes [`AppState::opds_server`]: the runtime owns the
/// listener, which owns the routes, which own the handler, which owns this
/// dependency set — holding the whole `AppState` would close that loop.
#[derive(Clone)]
pub struct OpdsServerDeps {
    db: SharedState,
    search_index: ArcMut<Option<Arc<SearchIndex>>>,
}

impl OpdsServerDeps {
    /// Build the dependency set for a running app state.
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self::new(state.db.clone(), state.search_index.clone())
    }

    /// Build the dependency set from the two handles it reads.
    ///
    /// The MCP bridge holds both already (see `crate::mcp::CommandBridge`), so it
    /// reaches the server without a second `AppState`.
    pub(crate) fn new(db: SharedState, search_index: ArcMut<Option<Arc<SearchIndex>>>) -> Self {
        Self { db, search_index }
    }

    /// Wrap the live repository in the handler the routes call.
    async fn handler(&self, base_url: String) -> Result<SharedOpdsHandler, OpdsServerError> {
        let index = {
            let guard = self.search_index.read().await;
            guard
                .clone()
                .ok_or_else(|| OpdsServerError::new("index", "search index is not available"))?
        };
        let repository: SharedOpdsRepository =
            Arc::new(LivtetOpdsRepository::from_state(&self.db, index));
        Ok(Arc::new(LiveOpdsHandler {
            service: OpdsService::new(repository, base_url),
        }))
    }
}

/// The [`OpdsHandler`] the route table calls.
///
/// `livtet-opds-server` ships [`OpdsService`] (the request-level facade) but no
/// [`OpdsHandler`] implementation over it, so the wiring end owns one adapter
/// and both the server and its tests go through it.
struct LiveOpdsHandler {
    service: OpdsService,
}

#[async_trait]
impl OpdsHandler for LiveOpdsHandler {
    async fn get_root_feed(&self) -> Result<Feed, OpdsError> {
        self.service.get_feed().await
    }

    async fn get_publication(&self, id: &str) -> Result<Publication, OpdsError> {
        self.service.get_publication(id).await
    }

    async fn search(&self, query: &str) -> Result<Feed, OpdsError> {
        self.service.search(query).await
    }

    async fn get_navigation_authors(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_authors().await
    }

    async fn get_navigation_tags(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_tags().await
    }

    async fn get_navigation_genres(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_genres().await
    }

    async fn get_linked_file(
        &self,
        work_id: &str,
        inventory_id: Option<&str>,
    ) -> Result<(Vec<u8>, String, String), OpdsError> {
        self.service.get_linked_file(work_id, inventory_id).await
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use livtet_core::data::entities::{digital_inventory, editions, works};
    use livtet_core::data::orm::{ActiveModelTrait, Set};
    use livtet_core::data::{Kind, TestDb};
    use livtet_core::search::SearchIndex;
    use livtet_types::{DbId, KnownFormats, now_primitive};
    use tokio::sync::RwLock;

    use super::{
        DEFAULT_PORT, OpdsServerDeps, OpdsServerError, OpdsServerRuntime, OpdsServerSettings,
        OpdsServerSettingsStore, desired_port,
    };

    /// Settings store that keeps the value in memory instead of on disk.
    #[derive(Default)]
    struct MemoryStore {
        value: std::sync::Mutex<Option<OpdsServerSettings>>,
    }

    impl MemoryStore {
        fn seeded(settings: OpdsServerSettings) -> Arc<Self> {
            Arc::new(Self {
                value: std::sync::Mutex::new(Some(settings)),
            })
        }
    }

    impl OpdsServerSettingsStore for MemoryStore {
        fn read(&self) -> Option<OpdsServerSettings> {
            self.value.lock().unwrap().clone()
        }

        fn write(&self, settings: &OpdsServerSettings) -> Result<(), String> {
            *self.value.lock().unwrap() = Some(settings.clone());
            Ok(())
        }
    }

    /// A port nothing is listening on, reserved by binding and releasing it.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("ephemeral port");
        let port = listener.local_addr().expect("bound address").port();
        drop(listener);
        port
    }

    const BOOK_BYTES: &[u8] = b"epub bytes served over OPDS";

    /// A catalog with one work, one edition, and one file on disk.
    struct Fixture {
        _db: TestDb,
        _dirs: Vec<tempfile::TempDir>,
        work_id: DbId,
        deps: OpdsServerDeps,
        store: Arc<MemoryStore>,
    }

    async fn fixture(port: u16) -> Fixture {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let books = tempfile::tempdir().expect("books dir");
        let book_path = camino::Utf8PathBuf::from_path_buf(books.path().join("bridge.epub"))
            .expect("utf8 book path");
        fs_err::write(&book_path, BOOK_BYTES).expect("write book");

        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("Bridge Work".to_string()),
            description: Set(None),
            sort_title: Set(None),
            series_type: Set(None),
            language_id: Set(None),
            preferred_edition_id: Set(None),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("work");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Bridge Edition".to_string())),
            published_date: Set(None),
            format_id: Set(Some(KnownFormats::Epub.into())),
            language_id: Set(None),
            notes: Set(None),
            description: Set(None),
            format_metadata: Set(None),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("edition");

        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(Some(book_path.to_string())),
            cover_path: Set(None),
            blurhash: Set(None),
            dominant_color: Set(None),
            file_hash: Set(None),
            file_size_bytes: Set(Some(BOOK_BYTES.len() as i64)),
            file_format: Set(Some("epub".to_string())),
            notes: Set(None),
            added_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("inventory");

        let index_dir = tempfile::tempdir().expect("index dir");
        let index_path = camino::Utf8Path::from_path(index_dir.path()).expect("utf8 index path");
        let index = SearchIndex::open(index_path).expect("open index");
        index.reindex(&db).await.expect("reindex");

        let deps = OpdsServerDeps {
            db: test_db.state().clone(),
            search_index: Arc::new(RwLock::new(Some(Arc::new(index)))),
        };

        Fixture {
            _db: test_db,
            _dirs: vec![books, index_dir],
            work_id,
            deps,
            store: MemoryStore::seeded(OpdsServerSettings {
                enabled: false,
                port,
            }),
        }
    }

    fn runtime(store: Arc<MemoryStore>) -> OpdsServerRuntime {
        OpdsServerRuntime::new(store)
    }

    /// GET `path` on the running listener, accepting the JSON OPDS dialect.
    async fn get(port: u16, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}{path}"))
            .header(reqwest::header::ACCEPT, "application/opds+json")
            .send()
            .await
            .expect("request reaches the OPDS server")
    }

    #[tokio::test]
    async fn status_starts_from_the_persisted_settings() {
        let runtime = runtime(MemoryStore::seeded(OpdsServerSettings {
            enabled: false,
            port: 9999,
        }));

        let status = runtime.status().await;
        assert_eq!(status.settings.port, 9999);
        assert!(!status.settings.enabled);
        assert!(!status.running);
        assert_eq!(status.url, None);
    }

    #[tokio::test]
    async fn an_unsaved_store_reads_as_the_off_by_default_baseline() {
        let runtime = runtime(Arc::new(MemoryStore::default()));

        let status = runtime.status().await;
        assert_eq!(status.settings.port, DEFAULT_PORT);
        assert!(!status.settings.enabled, "a fresh install opens no socket");
    }

    #[tokio::test]
    async fn save_refuses_an_ephemeral_port() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let store = MemoryStore::seeded(OpdsServerSettings {
            enabled: false,
            port: 8080,
        });
        let runtime = runtime(store.clone());
        let deps = OpdsServerDeps {
            db: test_db.state().clone(),
            search_index: Arc::new(RwLock::new(None)),
        };

        let error: OpdsServerError = runtime
            .save(
                OpdsServerSettings {
                    enabled: true,
                    port: 0,
                },
                &deps,
            )
            .await
            .expect_err("port 0 is refused");

        assert_eq!(error.code, "invalid-port");
        assert_eq!(
            store.read().expect("settings").port,
            8080,
            "a refused save writes nothing"
        );
        assert!(!runtime.status().await.running);
    }

    #[tokio::test]
    async fn start_without_a_search_index_reports_the_missing_index() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let runtime = runtime(MemoryStore::seeded(OpdsServerSettings {
            enabled: false,
            port: free_port(),
        }));
        let deps = OpdsServerDeps {
            db: test_db.state().clone(),
            search_index: Arc::new(RwLock::new(None)),
        };

        let error = runtime.start(&deps).await.expect_err("no index, no server");
        assert_eq!(error.code, "index");
        assert!(!runtime.status().await.running);
    }

    #[tokio::test]
    async fn start_serves_the_library_over_http_and_stop_unbinds_it() {
        let port = free_port();
        let fixture = fixture(port).await;
        let runtime = runtime(fixture.store.clone());

        let started = runtime.start(&fixture.deps).await.expect("server starts");
        assert!(started.running);
        let url = started
            .url
            .clone()
            .expect("a running server publishes its URL");
        assert!(
            url.ends_with(&format!(":{port}")),
            "the feed URL names the bound port: {url}"
        );
        assert!(
            fixture.store.read().expect("settings").enabled,
            "starting remembers the choice"
        );

        let feed = get(port, "/").await;
        assert_eq!(feed.status(), reqwest::StatusCode::OK);
        let body = feed.text().await.expect("feed body");
        assert!(
            body.contains(&fixture.work_id.to_string()),
            "the feed links the seeded work by id: {body}"
        );
        assert!(
            body.contains("Bridge Edition"),
            "the publication carries the edition title: {body}"
        );

        let download = get(port, &format!("/publications/{}/download", fixture.work_id)).await;
        assert_eq!(download.status(), reqwest::StatusCode::OK);
        assert_eq!(
            download.bytes().await.expect("book bytes").as_ref(),
            BOOK_BYTES
        );

        let stopped = runtime.stop(&fixture.deps).await.expect("server stops");
        assert!(!stopped.running);
        assert_eq!(stopped.url, None);
        assert!(
            reqwest::Client::new()
                .get(format!("http://127.0.0.1:{port}/"))
                .send()
                .await
                .is_err(),
            "a stopped server refuses connections"
        );
    }

    #[tokio::test]
    async fn saving_a_new_port_moves_the_listener() {
        let first = free_port();
        let fixture = fixture(first).await;
        let runtime = runtime(fixture.store.clone());
        runtime.start(&fixture.deps).await.expect("server starts");

        let second = free_port();
        let moved = runtime
            .save(
                OpdsServerSettings {
                    enabled: true,
                    port: second,
                },
                &fixture.deps,
            )
            .await
            .expect("server moves");

        assert_eq!(moved.settings.port, second);
        assert_eq!(get(second, "/").await.status(), reqwest::StatusCode::OK);
        assert!(
            reqwest::Client::new()
                .get(format!("http://127.0.0.1:{first}/"))
                .send()
                .await
                .is_err(),
            "the old port was released"
        );

        runtime.stop(&fixture.deps).await.expect("cleanup");
    }

    #[tokio::test]
    async fn a_port_that_cannot_bind_leaves_the_listener_and_the_store_alone() {
        let port = free_port();
        let fixture = fixture(port).await;
        let runtime = runtime(fixture.store.clone());
        runtime.start(&fixture.deps).await.expect("server starts");

        // Hold a port on every interface so the server's bind must fail.
        let held =
            std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, 0)).expect("hold a port");
        let taken = held.local_addr().expect("held address").port();

        let error = runtime
            .save(
                OpdsServerSettings {
                    enabled: true,
                    port: taken,
                },
                &fixture.deps,
            )
            .await
            .expect_err("a taken port is refused");
        assert_eq!(error.code, "bind");

        let status = runtime.status().await;
        assert!(status.running, "the running listener survived the refusal");
        assert!(
            status
                .url
                .as_deref()
                .is_some_and(|url| url.ends_with(&format!(":{port}"))),
            "still publishing the old port: {:?}",
            status.url
        );
        assert_eq!(
            fixture.store.read().expect("settings").port,
            port,
            "a refused save is not remembered, so the next launch can still start"
        );
        assert_eq!(
            get(port, "/").await.status(),
            reqwest::StatusCode::OK,
            "the original catalog still answers"
        );

        runtime.stop(&fixture.deps).await.expect("cleanup");
        drop(held);
    }

    #[tokio::test]
    async fn saving_the_running_configuration_keeps_the_listener() {
        let port = free_port();
        let fixture = fixture(port).await;
        let runtime = runtime(fixture.store.clone());
        runtime.start(&fixture.deps).await.expect("server starts");

        let again = runtime
            .save(
                OpdsServerSettings {
                    enabled: true,
                    port,
                },
                &fixture.deps,
            )
            .await
            .expect("idempotent save");
        assert!(again.running);
        assert_eq!(get(port, "/").await.status(), reqwest::StatusCode::OK);

        runtime.stop(&fixture.deps).await.expect("cleanup");
    }

    #[tokio::test]
    async fn a_hand_edited_store_with_port_zero_stays_off() {
        assert_eq!(
            desired_port(&OpdsServerSettings {
                enabled: true,
                port: 0
            }),
            None
        );
        assert_eq!(
            desired_port(&OpdsServerSettings {
                enabled: false,
                port: 8080
            }),
            None
        );
        assert_eq!(
            desired_port(&OpdsServerSettings {
                enabled: true,
                port: 8080
            }),
            Some(8080)
        );
    }

    #[test]
    fn a_feed_url_is_absolute_http() {
        let url = super::base_url(8080);
        assert!(url.starts_with("http://"), "{url}");
        assert!(url.ends_with(":8080"), "{url}");
        assert!(
            !url.contains("0.0.0.0"),
            "a wildcard bind is not an address a client can dial: {url}"
        );
    }
}
