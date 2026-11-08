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
use livtet_opds_live::LivtetOpdsRepository;
use livtet_opds_server::{
    OpdsHandler, OpdsServer, OpdsService, PoemOpdsServer, SharedOpdsHandler, SharedOpdsRepository,
    routes::make_routes,
};
use livtet_opds_types::{Feed, OpdsError, Publication};
use poem::endpoint::BoxEndpoint;
use poem::http::{Method, Uri, header};
use poem::{Body, Endpoint, Request, Response};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex;

use crate::types::{AppState, ArcMut};

/// Port the server binds when the user has not chosen one.
pub(crate) const DEFAULT_PORT: u16 = 8080;

/// Key the settings are stored under inside `opds-server.json`.
pub(crate) const SETTINGS_KEY: &str = "opds-server";

/// The origin the platform serves a paired desktop's catalog from.
///
/// A feed's links are absolute and name the LAN address, which a client that
/// reached us over iroh cannot dial; they are rewritten to this prefix. The
/// value may carry a path (`https://host/opds/<node>`), which is why it is a
/// prefix rather than a bare origin.
pub(crate) const DEFAULT_PLATFORM_ORIGIN: &str = "https://livtet.olamaelcu.net";

/// Environment override for [`DEFAULT_PLATFORM_ORIGIN`].
pub(crate) const PLATFORM_ORIGIN_ENV: &str = "LIVTET_OPDS_ORIGIN";

/// The origin forwarded links are rewritten to.
///
/// Read per construction rather than baked in, so a deployment on another
/// origin (or under another path) needs no rebuild.
pub(crate) fn platform_origin() -> String {
    std::env::var(PLATFORM_ORIGIN_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PLATFORM_ORIGIN.to_string())
}

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

/// The base URL a pass-through route table builds its links from.
///
/// The listener may be off — pass-through does not need it — so the stored
/// port is used whether or not anything is bound to it. A stored zero is
/// treated as the default, the same rule the listener applies.
pub(crate) fn link_base_url(runtime: &OpdsServerRuntime) -> String {
    let port = runtime.settings().port;
    base_url(if port == 0 { DEFAULT_PORT } else { port })
}

/// The machine's primary IPv4 address.
///
/// A UDP `connect` sends no packet: it asks the kernel which local address the
/// route to `192.0.2.1` (TEST-NET-1, never routed) would use, which is the
/// address an e-reader on the same network would reach this machine at.
///
/// Shared with the KOReader sync server, which publishes its base URL the same
/// way: the address a device on the same network would dial.
pub(crate) fn lan_ipv4() -> Option<Ipv4Addr> {
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

/// One OPDS request the sync daemon forwards over its stdio control channel.
///
/// The fields mirror the wire request (`method`, `path`, `headers`, `body`).
/// The type is local because the sidecar's wire crate is not a dependency of
/// this process: JSON is the contract between the two, and its shape is the
/// daemon's `Opds` request.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct OpdsRouteRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) headers: OpdsRequestHeaders,
    #[serde(default)]
    pub(crate) body: Vec<u8>,
}

/// The negotiation headers a forwarded request carries.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct OpdsRequestHeaders {
    pub(crate) accept: Option<String>,
    pub(crate) accept_language: Option<String>,
    pub(crate) accept_encoding: Option<String>,
}

/// The route's answer, shaped like the wire's `OpdsResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct OpdsRouteResponse {
    pub(crate) status: u16,
    pub(crate) content_type: String,
    pub(crate) body: Vec<u8>,
}

/// The OPDS route table, callable without a listener.
///
/// A paired platform's client cannot reach this machine's LAN address, so the
/// desktop answers through the sync daemon instead: the daemon forwards the
/// request, this runs it through the same `make_routes` table the LAN server
/// serves, and the answer goes back the same way. Nothing binds a port.
pub(crate) struct OpdsDispatch {
    endpoint: BoxEndpoint<'static, Response>,
    /// The base URL the route table builds absolute links from.
    base_url: String,
    /// The origin those links are rewritten to.
    target: String,
}

impl OpdsDispatch {
    /// Build the route table over the live repository.
    ///
    /// `base_url` is the address the routes believe they serve (the LAN base
    /// URL); [`OpdsDispatch::respond`] rewrites what they emit to `target`.
    pub(crate) async fn new(
        deps: &OpdsServerDeps,
        base_url: String,
        target: String,
    ) -> Result<Self, OpdsServerError> {
        let handler = deps.handler(base_url.clone()).await?;
        Ok(Self {
            endpoint: make_routes(handler, None),
            base_url,
            target,
        })
    }

    /// Serve one forwarded request and return the route's HTTP answer.
    ///
    /// A non-2xx status is returned as-is: the e-reader acts on the 404.
    pub(crate) async fn respond(
        &self,
        request: OpdsRouteRequest,
    ) -> Result<OpdsRouteResponse, String> {
        let OpdsRouteRequest {
            method,
            path,
            headers,
            body,
        } = request;
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|error| format!("opds: invalid method {method:?}: {error}"))?;
        let uri = path
            .parse::<Uri>()
            .map_err(|error| format!("opds: invalid path {path:?}: {error}"))?;

        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(accept) = headers.accept {
            builder = builder.header(header::ACCEPT.as_str(), accept);
        }
        if let Some(language) = headers.accept_language {
            builder = builder.header(header::ACCEPT_LANGUAGE.as_str(), language);
        }
        if let Some(encoding) = headers.accept_encoding {
            builder = builder.header(header::ACCEPT_ENCODING.as_str(), encoding);
        }

        // `get_response` converts a handler's error into its HTTP response
        // (a 404 for an unknown publication) instead of surfacing it as a
        // failure of the forward, which would erase the status the e-reader
        // needs.
        let response = self.endpoint.get_response(builder.body(Body::from(body))).await;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let body = response
            .into_body()
            .into_vec()
            .await
            .map_err(|error| format!("opds: reading the route body failed: {error}"))?;

        Ok(OpdsRouteResponse {
            status,
            content_type: content_type.clone(),
            body: self.rewrite(&content_type, body),
        })
    }

    /// Point absolute links at the origin the platform served the feed from.
    ///
    /// Only markup that can carry links is touched. A publication download is
    /// bytes — a PDF or an EPUB — and searching it for a URL would corrupt it,
    /// so the media type decides, never the content.
    fn rewrite(&self, content_type: &str, body: Vec<u8>) -> Vec<u8> {
        if self.base_url == self.target || !carries_links(content_type) {
            return body;
        }
        match String::from_utf8(body) {
            Ok(text) => text.replace(&self.base_url, &self.target).into_bytes(),
            // Not UTF-8 despite the media type: leave the bytes untouched
            // rather than transcode a document we cannot parse.
            Err(error) => error.into_bytes(),
        }
    }
}

/// Whether a media type is text that may carry absolute links.
///
/// The OPDS documents this server emits are Atom XML, OPDS JSON, and the
/// OpenSearch description; everything else is a publication or an image.
fn carries_links(content_type: &str) -> bool {
    let media = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    media.ends_with("+xml")
        || media.ends_with("+json")
        || matches!(
            media.as_str(),
            "application/xml" | "text/xml" | "application/json"
        )
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
        DEFAULT_PORT, OpdsDispatch, OpdsRequestHeaders, OpdsRouteRequest, OpdsServerDeps,
        OpdsServerError, OpdsServerRuntime, OpdsServerSettings, OpdsServerSettingsStore,
        carries_links, desired_port,
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
        fixture_with_book(port, BOOK_BYTES).await
    }

    async fn fixture_with_book(port: u16, book_bytes: &[u8]) -> Fixture {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let books = tempfile::tempdir().expect("books dir");
        let book_path = camino::Utf8PathBuf::from_path_buf(books.path().join("bridge.epub"))
            .expect("utf8 book path");
        fs_err::write(&book_path, book_bytes).expect("write book");

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
            cover_image_url: Set(None),
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
            file_size_bytes: Set(Some(book_bytes.len() as i64)),
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

    /// The LAN base URL the routes build links from, and the platform prefix
    /// those links must name instead.
    const LAN_BASE: &str = "http://192.168.1.5:8080";
    const FORWARDED_ORIGIN: &str = "https://livtet.olamaelcu.net/opds/node";

    fn route_request(method: &str, path: &str, accept: Option<&str>) -> OpdsRouteRequest {
        OpdsRouteRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers: OpdsRequestHeaders {
                accept: accept.map(str::to_string),
                ..OpdsRequestHeaders::default()
            },
            body: Vec::new(),
        }
    }

    async fn forwarded(fixture: &Fixture) -> OpdsDispatch {
        OpdsDispatch::new(
            &fixture.deps,
            LAN_BASE.to_string(),
            FORWARDED_ORIGIN.to_string(),
        )
        .await
        .expect("the route table builds over the live repository")
    }

    /// An unknown publication is the route's own 404; forwarding must not turn
    /// it into an error, because the e-reader acts on the status.
    #[tokio::test]
    async fn a_forwarded_route_404_comes_back_unchanged() {
        let fixture = fixture(DEFAULT_PORT).await;
        let dispatch = forwarded(&fixture).await;

        let answer = dispatch
            .respond(route_request(
                "GET",
                "/publications/no-such-work",
                Some("application/opds+json"),
            ))
            .await
            .expect("the route answers");

        assert_eq!(answer.status, 404);
        assert!(
            String::from_utf8(answer.body)
                .expect("the route's error body is text")
                .contains("not found"),
            "the route's own explanation survives the forward"
        );
    }

    /// Absolute links are built from the LAN base URL; a client that reached
    /// the platform must be sent back to the platform, not to the LAN.
    #[tokio::test]
    async fn forwarded_links_name_the_platform_origin() {
        let fixture = fixture(DEFAULT_PORT).await;
        let dispatch = forwarded(&fixture).await;

        let answer = dispatch
            .respond(route_request(
                "GET",
                "/",
                Some("application/atom+xml;profile=opds-catalog"),
            ))
            .await
            .expect("the feed renders");
        let body = String::from_utf8(answer.body).expect("an Atom feed is text");

        assert_eq!(answer.status, 200);
        assert!(
            !body.contains(LAN_BASE),
            "no link may keep the LAN address: {body}"
        );
        // Every absolute link the feed emits — its own `self` and the shelf
        // facet — must name the origin the client dialled, not the LAN.
        assert!(
            body.contains(&format!("{FORWARDED_ORIGIN}/search")),
            "the shelf link names the platform: {body}"
        );
        assert!(
            body.contains(FORWARDED_ORIGIN) && body.matches(FORWARDED_ORIGIN).count() >= 2,
            "more than one link names the platform: {body}"
        );
    }

    /// A publication download is bytes: even one whose contents happen to
    /// contain the LAN base URL must arrive byte for byte.
    #[tokio::test]
    async fn a_forwarded_download_is_never_rewritten() {
        let mut book = Vec::new();
        book.extend_from_slice(b"%PDF-1.7\n% ");
        book.extend_from_slice(LAN_BASE.as_bytes());
        book.extend_from_slice(b"\n%%EOF\n");
        let fixture = fixture_with_book(DEFAULT_PORT, &book).await;
        let dispatch = forwarded(&fixture).await;

        let answer = dispatch
            .respond(route_request(
                "GET",
                &format!("/publications/{}/download", fixture.work_id),
                None,
            ))
            .await
            .expect("the download is served");

        assert_eq!(answer.status, 200);
        assert_eq!(
            answer.body, book,
            "a binary body is passed through untouched"
        );
        assert_eq!(answer.content_type, "application/epub+zip");
    }

    #[test]
    fn only_link_carrying_markup_is_rewritten() {
        assert!(carries_links("application/atom+xml;profile=opds-catalog"));
        assert!(carries_links("application/opds+json"));
        assert!(carries_links("application/opensearchdescription+xml"));
        assert!(!carries_links("application/epub+zip"));
        assert!(!carries_links("application/pdf"));
        assert!(!carries_links("image/jpeg"));
        assert!(!carries_links("application/octet-stream"));
    }
}
