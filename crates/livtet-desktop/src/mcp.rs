//! Localhost-only Model Context Protocol endpoint for a curated subset of the
//! desktop commands: search, import, and reader.
//!
//! Configured from Settings (`commands::mcp`) and persisted in `mcp.json`
//! (ADR-0041). The endpoint runs inside the application process over the live
//! database, search index, and reader cache, so a tool call executes exactly
//! the code the webview reaches over IPC — no second process, no socket
//! bridge, and no second database connection that could race the app's own
//! writes.
//!
//! Authorization is the bind address: `127.0.0.1` only, plus the transport's
//! loopback `Host` allowlist (which is what stops a web page in a browser from
//! driving the endpoint through DNS rebinding). There is no token; a remote
//! caller needs an SSH tunnel, which is already authenticated.

use std::sync::Arc;

use camino::Utf8PathBuf;
use livtet_core::data::SharedState;
use livtet_core::search::SearchIndex;
use livtet_mcp_server::{BridgeError, DesktopBridge, McpEndpoint};
use livtet_types::DbId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tokio::sync::Mutex;

use crate::commands::import::{
    AuthorRequirement, ImportContext, ImportMode, VirtualBookInput, create_virtual_book_and_index,
    import_batch, import_one_inner,
};
use crate::commands::reader::{
    ReaderCache, delete_reader_bookmark, fetch_reader_bookmarks, fetch_reader_position,
    publication_for_edition, resource_text_for_edition, store_reader_bookmark,
    store_reader_position,
};
use crate::commands::search::{
    EditionFilters, filter_options_inner, search_editions_inner, search_typeahead_inner,
};
use crate::opds_server::{OpdsServerDeps, OpdsServerError, OpdsServerSettings};
use crate::types::{AppState, ArcMut};

/// The environment variable that overrides the stored settings.
///
/// It exists for development and tests: when it names a port the endpoint is on
/// at that port whatever Settings says, so a launch can expose the endpoint
/// without touching the user's saved configuration.
const PORT_ENV: &str = "LIVTET_MCP_PORT";

/// How the endpoint is configured, persisted in `mcp.json` under [`SETTINGS_KEY`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct McpSettings {
    /// Whether the endpoint listens at all.
    pub enabled: bool,
    /// Loopback port to bind.
    pub port: u16,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: livtet_mcp_server::DEFAULT_PORT,
        }
    }
}

/// What Settings shows: the stored configuration plus what is actually running.
#[derive(Debug, Clone, Serialize, Type)]
pub struct McpStatus {
    /// The persisted configuration.
    pub settings: McpSettings,
    /// Whether a listener is bound right now.
    pub running: bool,
    /// The URL a client connects to, when a listener is bound.
    pub url: Option<String>,
    /// Port forced by [`PORT_ENV`], which overrides the stored one.
    pub env_port: Option<u16>,
}

/// A settings failure, shaped like the other command errors (`{ code, message }`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct McpSettingsError {
    /// Stable machine-readable reason: `invalid-port`, `storage`, `bind`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl McpSettingsError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Where the settings live. The trait exists so tests and the `AppState`
/// fixture can supply an in-memory store instead of a Tauri store file.
pub(crate) trait McpSettingsStore: Send + Sync {
    /// The persisted settings, absent until the first save.
    fn read(&self) -> Option<McpSettings>;
    /// Replace the persisted settings and flush them.
    fn write(&self, settings: &McpSettings) -> Result<(), String>;
}

/// Key the settings are stored under inside `mcp.json`.
pub(crate) const SETTINGS_KEY: &str = "mcp";

impl McpSettingsStore for tauri_plugin_store::Store<tauri::Wry> {
    fn read(&self) -> Option<McpSettings> {
        self.get(SETTINGS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    fn write(&self, settings: &McpSettings) -> Result<(), String> {
        let value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
        self.set(SETTINGS_KEY, value);
        self.save().map_err(|error| error.to_string())
    }
}

/// The live endpoint plus the settings that drive it.
///
/// Held on [`AppState`] so the Settings commands can start, stop, or move the
/// listener without restarting the app.
pub struct McpRuntime {
    store: Arc<dyn McpSettingsStore>,
    endpoint: Mutex<Option<McpEndpoint>>,
}

impl McpRuntime {
    /// Build the runtime over a settings store; nothing is bound until
    /// [`McpRuntime::apply`] runs.
    pub(crate) fn new(store: Arc<dyn McpSettingsStore>) -> Self {
        Self {
            store,
            endpoint: Mutex::new(None),
        }
    }

    /// The persisted settings, or the off-by-default baseline.
    pub(crate) fn settings(&self) -> McpSettings {
        self.store.read().unwrap_or_default()
    }

    /// The stored configuration plus what is actually running.
    pub(crate) async fn status(&self) -> McpStatus {
        let endpoint = self.endpoint.lock().await;
        McpStatus {
            settings: self.settings(),
            running: endpoint.is_some(),
            url: endpoint.as_ref().map(McpEndpoint::url),
            env_port: endpoint_port(std::env::var(PORT_ENV).ok().as_deref()),
        }
    }

    /// Persist `settings`, then reconcile the listener with them.
    ///
    /// A port of 0 is refused: it would bind an ephemeral port the user cannot
    /// configure a client against.
    pub(crate) async fn save(
        &self,
        settings: McpSettings,
        bridge: CommandBridge,
    ) -> Result<McpStatus, McpSettingsError> {
        if settings.port == 0 {
            return Err(McpSettingsError::new(
                "invalid-port",
                "port must be between 1 and 65535",
            ));
        }

        self.store
            .write(&settings)
            .map_err(|message| McpSettingsError::new("storage", message))?;
        self.apply(&settings, bridge).await?;
        Ok(self.status().await)
    }

    /// Reconcile the listener with `settings`, honouring [`PORT_ENV`].
    pub(crate) async fn apply(
        &self,
        settings: &McpSettings,
        bridge: CommandBridge,
    ) -> Result<(), McpSettingsError> {
        let env = endpoint_port(std::env::var(PORT_ENV).ok().as_deref());
        self.apply_with(settings, bridge, env).await
    }

    /// [`McpRuntime::apply`] with the environment override supplied, so tests
    /// do not read (or mutate) the process environment.
    async fn apply_with(
        &self,
        settings: &McpSettings,
        bridge: CommandBridge,
        env: Option<u16>,
    ) -> Result<(), McpSettingsError> {
        let desired = desired_port(settings, env);

        let mut endpoint = self.endpoint.lock().await;
        // Already serving the port the caller wants: leave the listener (and
        // any connected client) alone.
        if let (Some(port), Some(running)) = (desired, endpoint.as_ref())
            && port != 0
            && running.local_addr().port() == port
        {
            return Ok(());
        }
        if let Some(running) = endpoint.take() {
            running.shutdown();
        }

        let Some(port) = desired else {
            tracing::info!("MCP endpoint stopped");
            return Ok(());
        };

        match livtet_mcp_server::serve(bridge, port).await {
            Ok(running) => {
                tracing::info!(url = %running.url(), "MCP endpoint listening on loopback");
                *endpoint = Some(running);
                Ok(())
            }
            Err(error) => Err(McpSettingsError::new("bind", error.to_string())),
        }
    }
}

/// Start the endpoint from persisted settings and any environment override.
///
/// Called once during app setup. A failure is logged, not fatal: the app runs
/// without the endpoint either way.
pub(crate) async fn start_from_settings(runtime: &McpRuntime, bridge: CommandBridge) {
    let settings = runtime.settings();
    if let Err(error) = runtime.apply(&settings, bridge).await {
        tracing::warn!(
            code = %error.code,
            message = %error.message,
            "MCP endpoint did not start"
        );
    }
}

/// The port to bind, or `None` when the endpoint should be off.
///
/// Precedence: [`PORT_ENV`] wins over the stored setting in both directions —
/// it can switch the endpoint on when Settings has it off (a dev launch) and
/// move it to another port.
fn desired_port(settings: &McpSettings, env: Option<u16>) -> Option<u16> {
    match env {
        Some(port) => Some(port),
        None if settings.enabled => Some(settings.port),
        None => None,
    }
}

/// The port named by `raw`, or `None` when the override is unset or malformed.
fn endpoint_port(raw: Option<&str>) -> Option<u16> {
    let raw = raw?.trim();
    // An empty value means "on, conventional port"; any other value must be a
    // port, so a typo leaves the override inert rather than binding something
    // unintended.
    if raw.is_empty() {
        return Some(livtet_mcp_server::DEFAULT_PORT);
    }
    match raw.parse::<u16>() {
        Ok(0) | Err(_) => {
            tracing::warn!(value = %raw, "ignoring {PORT_ENV}: expected a TCP port");
            None
        }
        Ok(port) => Some(port),
    }
}

/// The bridge over the parts of [`AppState`] a tool call needs.
///
/// It deliberately excludes [`AppState::mcp`]: the runtime owns the endpoint,
/// which owns the serving task, which owns this bridge — holding the whole
/// `AppState` would close that loop into a leak.
#[derive(Clone)]
pub(crate) struct CommandBridge {
    db: SharedState,
    search_index: ArcMut<Option<Arc<SearchIndex>>>,
    covers_dir: Utf8PathBuf,
    opds_http: reqwest::Client,
    books_dir: Utf8PathBuf,
    plugin_host_path: Utf8PathBuf,
    plugin_host_config: Utf8PathBuf,
    plugins_dir: Utf8PathBuf,
    readers: Arc<ReaderCache>,
    loopback: crate::commands::byte_server::LoopbackServer,
    /// The OPDS catalog server, so a tool call can report or move it. Distinct
    /// from [`AppState::mcp`] in the doc above: the OPDS runtime owns a listener
    /// and settings, never this bridge, so holding it closes no loop.
    opds_server: Arc<crate::opds_server::OpdsServerRuntime>,
}

impl CommandBridge {
    /// Build the bridge for a running app state.
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            db: state.db.clone(),
            search_index: state.search_index.clone(),
            covers_dir: state.covers_dir.clone(),
            opds_http: state.opds_http.clone(),
            books_dir: state.books_dir.clone(),
            plugin_host_path: state.plugin_host_path.clone(),
            plugin_host_config: state.plugin_host_config.clone(),
            plugins_dir: state.plugins_dir.clone(),
            readers: state.readers.clone(),
            loopback: state.loopback.clone(),
            opds_server: state.opds_server.clone(),
        }
    }

    /// The import pipeline's view of the library.
    fn import_context(&self) -> ImportContext<'_> {
        ImportContext {
            db: &self.db,
            covers_dir: &self.covers_dir,
            books_dir: &self.books_dir,
            search_index: &self.search_index,
            plugin_host_path: &self.plugin_host_path,
            plugin_host_config: &self.plugin_host_config,
            plugins_dir: &self.plugins_dir,
        }
    }
}

impl DesktopBridge for CommandBridge {
    async fn search_typeahead(
        &self,
        query: String,
        limit: Option<i32>,
    ) -> Result<Value, BridgeError> {
        let index = self.search_index.read().await;
        let hits = search_typeahead_inner(index.as_deref(), &query, limit)
            .await
            .map_err(|error| bridge_error("search", &error))?;
        serialized(hits)
    }

    async fn search_editions(
        &self,
        query: Option<String>,
        filters: Option<Value>,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, BridgeError> {
        let filters = match filters {
            Some(value) => Some(
                serde_json::from_value::<EditionFilters>(value)
                    .map_err(|error| BridgeError::new("invalid-parameter", error.to_string()))?,
            ),
            None => None,
        };

        let index = self.search_index.read().await;
        let response = search_editions_inner(
            index.as_deref(),
            &self.db.db_conn(),
            query.as_deref(),
            filters,
            offset,
            limit,
        )
        .await
        .map_err(|error| bridge_error("search", &error))?;
        serialized(response)
    }

    async fn filter_options(&self) -> Result<Value, BridgeError> {
        let options = filter_options_inner(&self.db.db_conn())
            .await
            .map_err(|error| bridge_error("search", &error))?;
        serialized(options)
    }

    async fn import_file(&self, path: String, mode: String) -> Result<Value, BridgeError> {
        let mode = import_mode(&mode)?;
        let context = self.import_context();
        let outcome = import_one_inner(&path, mode, &context)
            .await
            .map_err(|error| bridge_error("import", &error))?;
        serialized(outcome)
    }

    async fn import_files(&self, paths: Vec<String>, mode: String) -> Result<Value, BridgeError> {
        let mode = import_mode(&mode)?;
        let context = self.import_context();
        // Progress events belong to the webview; an MCP caller gets only the
        // aggregate result.
        let result = import_batch(paths, mode, &context, |_| {}, |_| {}).await;
        serialized(result)
    }

    async fn create_virtual_book(&self, input: Value) -> Result<Value, BridgeError> {
        let input = serde_json::from_value::<VirtualBookInput>(input)
            .map_err(|error| BridgeError::new("invalid-parameter", error.to_string()))?;
        let outcome = create_virtual_book_and_index(
            &self.db,
            &self.covers_dir,
            &self.opds_http,
            &self.search_index,
            input,
            AuthorRequirement::Required,
        )
        .await
        .map_err(|error| bridge_error("import", &error))?;
        serialized(outcome)
    }

    async fn reader_publication(&self, edition_id: String) -> Result<Value, BridgeError> {
        let publication = publication_for_edition(
            &self.db.db_conn(),
            &self.readers,
            &self.loopback,
            &edition_id,
        )
        .await
        .map_err(|error| bridge_error("reader", &error))?;
        serialized(publication)
    }

    async fn reader_resource(
        &self,
        edition_id: String,
        href: String,
    ) -> Result<Value, BridgeError> {
        let text = resource_text_for_edition(&self.db.db_conn(), &self.readers, &edition_id, &href)
            .await
            .map_err(|error| bridge_error("reader", &error))?;
        serialized(text)
    }

    async fn get_reader_position(&self, edition_id: String) -> Result<Value, BridgeError> {
        let id = edition_id_arg(&edition_id)?;
        let position = fetch_reader_position(&self.db.db_conn(), id)
            .await
            .map_err(|error| bridge_error("reader", &error))?;
        serialized(position)
    }

    async fn save_reader_position(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        annotations: Option<String>,
    ) -> Result<Value, BridgeError> {
        let id = edition_id_arg(&edition_id)?;
        store_reader_position(
            &self.db.db_conn(),
            id,
            &kind,
            locator,
            page_number,
            position_seconds,
            annotations,
        )
        .await
        .map_err(|error| bridge_error("reader", &error))?;
        Ok(Value::Null)
    }

    async fn list_reader_bookmarks(&self, edition_id: String) -> Result<Value, BridgeError> {
        let id = edition_id_arg(&edition_id)?;
        let bookmarks = fetch_reader_bookmarks(&self.db.db_conn(), id)
            .await
            .map_err(|error| bridge_error("reader", &error))?;
        serialized(bookmarks)
    }

    async fn add_reader_bookmark(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        fraction: Option<f64>,
    ) -> Result<Value, BridgeError> {
        let id = edition_id_arg(&edition_id)?;
        let bookmark = store_reader_bookmark(
            &self.db.db_conn(),
            id,
            &kind,
            locator,
            page_number,
            position_seconds,
            fraction,
        )
        .await
        .map_err(|error| bridge_error("reader", &error))?;
        serialized(bookmark)
    }

    async fn remove_reader_bookmark(&self, bookmark_id: String) -> Result<Value, BridgeError> {
        let id = bookmark_id_arg(&bookmark_id)?;
        let removed = delete_reader_bookmark(&self.db.db_conn(), id)
            .await
            .map_err(|error| bridge_error("reader", &error))?;
        serialized(removed)
    }

    async fn opds_server_status(&self) -> Result<Value, BridgeError> {
        serialized(self.opds_server.status().await)
    }

    async fn opds_server_settings_save(&self, settings: Value) -> Result<Value, BridgeError> {
        let settings: OpdsServerSettings = serde_json::from_value(settings)
            .map_err(|error| BridgeError::new("invalid-parameter", error.to_string()))?;
        let status = self
            .opds_server
            .save(settings, &self.opds_deps())
            .await
            .map_err(opds_server_error)?;
        serialized(status)
    }

    async fn opds_server_start(&self) -> Result<Value, BridgeError> {
        let status = self
            .opds_server
            .start(&self.opds_deps())
            .await
            .map_err(opds_server_error)?;
        serialized(status)
    }

    async fn opds_server_stop(&self) -> Result<Value, BridgeError> {
        let status = self
            .opds_server
            .stop(&self.opds_deps())
            .await
            .map_err(opds_server_error)?;
        serialized(status)
    }
}

impl CommandBridge {
    /// The server's view of the library: the same pool and index it already holds.
    fn opds_deps(&self) -> OpdsServerDeps {
        OpdsServerDeps::new(self.db.clone(), self.search_index.clone())
    }
}

/// Render an OPDS settings failure under its own stable code (`bind`,
/// `invalid-port`, `index`, `storage`, `stop`), so a client can branch on it
/// exactly as the Settings panel does.
fn opds_server_error(error: OpdsServerError) -> BridgeError {
    BridgeError::new(error.code, error.message)
}

/// Parse an edition id, matching the command's own `invalid-id` code.
fn edition_id_arg(raw: &str) -> Result<DbId, BridgeError> {
    raw.parse::<DbId>()
        .map_err(|_| BridgeError::new("invalid-id", "edition id is invalid"))
}

/// Parse a bookmark id, matching the command's own `invalid-id` code.
fn bookmark_id_arg(raw: &str) -> Result<DbId, BridgeError> {
    raw.parse::<DbId>()
        .map_err(|_| BridgeError::new("invalid-id", "bookmark id is invalid"))
}

/// Map the wire spelling of an import mode onto the command's enum.
fn import_mode(raw: &str) -> Result<ImportMode, BridgeError> {
    match raw {
        "link" => Ok(ImportMode::Link),
        "copy" => Ok(ImportMode::Copy),
        other => Err(BridgeError::new(
            "invalid-parameter",
            format!("unknown import mode {other:?}; expected \"link\" or \"copy\""),
        )),
    }
}

/// Serialize a command result for the endpoint.
fn serialized<T: Serialize>(value: T) -> Result<Value, BridgeError> {
    serde_json::to_value(value)
        .map_err(|error| BridgeError::new("serialization", error.to_string()))
}

/// Render a desktop error for an MCP client under the bridge's own code.
fn bridge_error<E: Serialize>(code: &str, error: &E) -> BridgeError {
    BridgeError::new(code, describe(error))
}

/// The command's own `message` when its payload carries one, otherwise the
/// whole payload: `ReaderError` and `ImportError` both expose `message`, while
/// `SearchIndexError` is an enum with no such field.
fn describe<E: Serialize>(error: &E) -> String {
    match serde_json::to_value(error) {
        Ok(Value::Object(fields)) => match fields.get("message").and_then(Value::as_str) {
            Some(message) => message.to_owned(),
            None => Value::Object(fields).to_string(),
        },
        Ok(other) => other.to_string(),
        Err(_) => String::from("command failed"),
    }
}

#[cfg(test)]
mod tests {
    // Test assertions: a panic is the failure signal.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::sync::Arc;

    use livtet_core::data::entities::{editions, works};
    use livtet_core::data::orm::{ActiveModelTrait, EntityTrait, PaginatorTrait, Set};
    use livtet_core::data::{Kind, TestDb};
    use livtet_types::{KnownFormats, now_primitive};
    use serde_json::json;

    use super::*;
    use crate::test_fixture::MemoryMcpSettings;

    /// A runtime over an in-memory store plus a bridge over the fixture
    /// `AppState`: real command code, in-memory database, no search index
    /// loaded (the fixture never opens one).
    /// Both halves must outlive the test: `TestDb` owns the temporary directory
    /// holding the SQLite file the bridge's pool points into, so dropping it
    /// early leaves the pool opening a deleted path (`SQLITE_CANTOPEN`).
    async fn harness() -> (TestDb, McpRuntime, CommandBridge) {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = crate::mock_state_for_bulk(&test_db).await;
        let runtime = McpRuntime::new(Arc::new(MemoryMcpSettings::default()));
        (test_db, runtime, CommandBridge::from_state(&state))
    }

    #[test]
    fn the_endpoint_is_off_until_it_is_enabled() {
        let settings = McpSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.port, livtet_mcp_server::DEFAULT_PORT);
        assert_eq!(desired_port(&settings, None), None);
    }

    #[test]
    fn the_environment_override_wins_in_both_directions() {
        let off = McpSettings {
            enabled: false,
            port: 7000,
        };
        let on = McpSettings {
            enabled: true,
            port: 7000,
        };
        // Switched on though Settings says off.
        assert_eq!(desired_port(&off, Some(7777)), Some(7777));
        // Moved though Settings names another port.
        assert_eq!(desired_port(&on, Some(7777)), Some(7777));
        // No override: the stored setting decides.
        assert_eq!(desired_port(&on, None), Some(7000));
    }

    #[test]
    fn the_port_override_ignores_malformed_values() {
        assert_eq!(endpoint_port(None), None);
        assert_eq!(endpoint_port(Some("7777")), Some(7777));
        assert_eq!(endpoint_port(Some(" 7777 ")), Some(7777));
        assert_eq!(
            endpoint_port(Some("")),
            Some(livtet_mcp_server::DEFAULT_PORT)
        );
        assert_eq!(endpoint_port(Some("http")), None);
        assert_eq!(endpoint_port(Some("70000")), None);
        assert_eq!(endpoint_port(Some("0")), None);
    }

    #[tokio::test]
    async fn saving_settings_persists_them() {
        let (_test_db, runtime, bridge) = harness().await;
        let saved = McpSettings {
            enabled: false,
            port: 8123,
        };

        let status = runtime
            .save(saved.clone(), bridge)
            .await
            .expect("save settings");
        assert_eq!(status.settings, saved);
        assert_eq!(runtime.settings(), saved);
        assert!(!status.running);
        assert_eq!(status.url, None);
    }

    #[tokio::test]
    async fn a_zero_port_is_refused_and_nothing_is_stored() {
        let (_test_db, runtime, bridge) = harness().await;

        let error = runtime
            .save(
                McpSettings {
                    enabled: true,
                    port: 0,
                },
                bridge,
            )
            .await
            .expect_err("port 0 has no URL to configure a client against");
        assert_eq!(error.code, "invalid-port");
        assert_eq!(runtime.settings(), McpSettings::default());
    }

    #[tokio::test]
    async fn applying_settings_starts_and_stops_the_listener() {
        let (_test_db, runtime, bridge) = harness().await;
        let enabled = McpSettings {
            enabled: true,
            // Ephemeral, so the test never collides with a real service.
            port: 0,
        };

        runtime
            .apply_with(&enabled, bridge.clone(), None)
            .await
            .expect("start the endpoint");
        let running = runtime.status().await;
        assert!(running.running);
        let url = running.url.expect("a running endpoint has a URL");
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(url.ends_with("/mcp"), "{url}");

        runtime
            .apply_with(&McpSettings::default(), bridge, None)
            .await
            .expect("stop the endpoint");
        let stopped = runtime.status().await;
        assert!(!stopped.running);
        assert_eq!(stopped.url, None);
    }

    #[tokio::test]
    async fn blank_typeahead_short_circuits_before_the_index() {
        let (_test_db, _runtime, bridge) = harness().await;
        let hits = bridge
            .search_typeahead(String::new(), None)
            .await
            .expect("blank query is never an error");
        assert_eq!(hits, json!([]));
    }

    #[tokio::test]
    async fn typeahead_maps_an_absent_index_to_a_search_error() {
        let (_test_db, _runtime, bridge) = harness().await;
        let error = bridge
            .search_typeahead(String::from("dune"), None)
            .await
            .expect_err("no index is loaded");
        assert_eq!(error.code, "search");
        assert!(error.message.contains("Unavailable"), "{error:?}");
    }

    #[tokio::test]
    async fn filter_options_reads_every_axis_from_the_catalog() {
        let (_test_db, _runtime, bridge) = harness().await;
        let options = bridge.filter_options().await.expect("read axes");
        for axis in [
            "formats",
            "languages",
            "tags",
            "genres",
            "subjects",
            "publishers",
            "authors",
        ] {
            assert!(
                options.get(axis).is_some(),
                "missing axis {axis}: {options}"
            );
        }
    }

    #[tokio::test]
    async fn garbage_edition_ids_are_rejected_before_any_query() {
        let (_test_db, _runtime, bridge) = harness().await;
        let error = bridge
            .get_reader_position(String::from("not-an-id"))
            .await
            .expect_err("unparseable id");
        assert_eq!(error.code, "invalid-id");
    }

    #[tokio::test]
    async fn unknown_import_modes_are_rejected() {
        let (_test_db, _runtime, bridge) = harness().await;
        let error = bridge
            .import_file(String::from("/tmp/book.epub"), String::from("sideways"))
            .await
            .expect_err("unknown mode");
        assert_eq!(error.code, "invalid-parameter");
    }

    #[tokio::test]
    async fn an_unknown_edition_describes_as_no_publication() {
        let (_test_db, _runtime, bridge) = harness().await;
        let publication = bridge
            .reader_publication(DbId::new().to_string())
            .await
            .expect("an unknown edition is not an error");
        assert_eq!(publication, Value::Null);
    }

    /// A bridge over a catalog holding one readable EPUB edition.
    ///
    /// Reader state is keyed by `(edition, format)`, and an edition with no
    /// `digital_inventory` row is not a virtual book, so nothing needs a file on
    /// disk. The `TestDb` is returned so it outlives the test: the fixture state
    /// shares its pool, and dropping it would close the database underneath.
    async fn seeded_catalog() -> (TestDb, CommandBridge, DbId) {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("Bridge Work".into()),
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
        .unwrap();

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Bridge Edition".into())),
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
        .unwrap();

        let state = crate::mock_state_for_bulk(&test_db).await;
        (test_db, CommandBridge::from_state(&state), edition_id)
    }

    #[tokio::test]
    async fn a_pdf_position_saves_and_reads_back() {
        let (_db, bridge, edition_id) = seeded_catalog().await;

        let saved = bridge
            .save_reader_position(
                edition_id.to_string(),
                String::from("pdf"),
                None,
                Some(12),
                None,
                None,
            )
            .await
            .expect("save a pdf position");
        assert_eq!(saved, Value::Null, "a position save returns nothing");

        let position = bridge
            .get_reader_position(edition_id.to_string())
            .await
            .expect("read it back");
        assert_eq!(position["kind"], json!("pdf"));
        assert_eq!(position["page_number"], json!(12));
        assert_eq!(position["locator"], Value::Null);
    }

    #[tokio::test]
    async fn an_epub_position_keeps_its_locator() {
        let (_db, bridge, edition_id) = seeded_catalog().await;
        let locator = String::from(r#"{"href":"/ch1.xhtml","locations":{"progression":0.4}}"#);

        bridge
            .save_reader_position(
                edition_id.to_string(),
                String::from("epub"),
                Some(locator.clone()),
                None,
                None,
                None,
            )
            .await
            .expect("save an epub position");

        let position = bridge
            .get_reader_position(edition_id.to_string())
            .await
            .expect("read it back");
        assert_eq!(position["kind"], json!("epub"));
        assert_eq!(position["locator"], json!(locator));
    }

    #[tokio::test]
    async fn a_position_save_rejects_an_unknown_kind() {
        let (_db, bridge, edition_id) = seeded_catalog().await;

        let error = bridge
            .save_reader_position(
                edition_id.to_string(),
                String::from("scroll"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect_err("scroll is not a reader family");
        assert_eq!(error.code, "reader");
        assert!(error.message.contains("kind must be"), "{error:?}");
    }

    #[tokio::test]
    async fn a_save_for_an_unknown_edition_reports_not_found() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .save_reader_position(
                DbId::new().to_string(),
                String::from("pdf"),
                None,
                Some(3),
                None,
                None,
            )
            .await
            .expect_err("nothing to save against");
        assert_eq!(error.code, "reader");
        assert!(error.message.contains("edition not found"), "{error:?}");
    }

    #[tokio::test]
    async fn bookmarks_add_list_and_remove() {
        let (_db, bridge, edition_id) = seeded_catalog().await;

        let added = bridge
            .add_reader_bookmark(
                edition_id.to_string(),
                String::from("pdf"),
                None,
                Some(12),
                None,
                None,
            )
            .await
            .expect("bookmark a page");
        let bookmark_id = added["id"]
            .as_str()
            .expect("a stored bookmark has an id")
            .to_owned();
        assert_eq!(added["kind"], json!("pdf"));
        assert_eq!(added["page_number"], json!(12));

        let listed = bridge
            .list_reader_bookmarks(edition_id.to_string())
            .await
            .expect("list them");
        assert_eq!(listed.as_array().map(Vec::len), Some(1), "{listed}");

        let removed = bridge
            .remove_reader_bookmark(bookmark_id.clone())
            .await
            .expect("remove it");
        assert_eq!(removed, json!(true));

        // A second click is a no-op, not an error.
        let again = bridge
            .remove_reader_bookmark(bookmark_id)
            .await
            .expect("removing it twice is fine");
        assert_eq!(again, json!(false));

        let after = bridge
            .list_reader_bookmarks(edition_id.to_string())
            .await
            .expect("list them again");
        assert_eq!(after, json!([]));
    }

    #[tokio::test]
    async fn an_epub_bookmark_needs_a_fraction() {
        let (_db, bridge, edition_id) = seeded_catalog().await;

        let error = bridge
            .add_reader_bookmark(
                edition_id.to_string(),
                String::from("epub"),
                Some(String::from(r#"{"href":"/ch1.xhtml"}"#)),
                None,
                None,
                None,
            )
            .await
            .expect_err("a fraction places the page in the book");
        assert_eq!(error.code, "reader");
        assert!(error.message.contains("fraction"), "{error:?}");
    }

    /// A bridge whose search index has been rebuilt from the seeded catalog.
    ///
    /// The temp dir is returned so the index survives the test.
    async fn indexed_catalog() -> (tempfile::TempDir, TestDb, CommandBridge) {
        let (test_db, _plain, _edition) = seeded_catalog().await;

        let index_dir = tempfile::tempdir().expect("index dir");
        let path = camino::Utf8Path::from_path(index_dir.path()).expect("utf-8 path");
        let index = livtet_core::search::SearchIndex::open(path).expect("open index");
        index
            .reindex(&test_db.state().db_conn())
            .await
            .expect("reindex the catalog");

        let state = crate::mock_state_for_bulk(&test_db).await;
        *state.search_index.write().await = Some(Arc::new(index));
        let bridge = CommandBridge::from_state(&state);
        (index_dir, test_db, bridge)
    }

    #[tokio::test]
    async fn search_editions_reports_an_unavailable_index() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .search_editions(None, None, None, None)
            .await
            .expect_err("the fixture loads no index");
        assert_eq!(error.code, "search");
        assert!(error.message.contains("Unavailable"), "{error:?}");
    }

    #[tokio::test]
    async fn search_editions_answers_from_an_indexed_catalog() {
        let (_index_dir, _db, bridge) = indexed_catalog().await;

        let response = bridge
            .search_editions(None, None, None, None)
            .await
            .expect("an indexed catalog answers");
        assert_eq!(response["total"], json!(1), "{response}");
        let hits = response["hits"].as_array().expect("hits are an array");
        assert_eq!(hits.len(), 1, "{response}");
    }

    #[tokio::test]
    async fn malformed_filters_report_invalid_parameter() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .search_editions(None, Some(json!({ "tag_ids": "not-a-list" })), None, None)
            .await
            .expect_err("filters are parsed before the query");
        assert_eq!(error.code, "invalid-parameter");
    }

    #[tokio::test]
    async fn a_virtual_book_is_created_without_a_file() {
        let (db, bridge, _edition) = seeded_catalog().await;
        let conn = db.state().db_conn();
        let before = editions::Entity::find()
            .count(&conn)
            .await
            .expect("count editions");

        let outcome = bridge
            .create_virtual_book(json!({
                "title": "Catalog Only",
                "authors": [{ "name": "A. Author" }],
            }))
            .await
            .expect("create a catalog-only book");
        assert!(outcome.is_object(), "{outcome}");

        let after = editions::Entity::find()
            .count(&conn)
            .await
            .expect("count editions");
        assert_eq!(after, before + 1, "the book reached the catalog");
    }

    #[tokio::test]
    async fn a_virtual_book_without_an_author_is_refused() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .create_virtual_book(json!({ "title": "Anonymous" }))
            .await
            .expect_err("a catalog-only book still needs a contributor");
        assert_eq!(error.code, "import");
    }

    // Multi-threaded like the app: `fs-err` formats a missing path through
    // `block_in_place`, which panics on a current-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn importing_a_missing_file_reports_an_import_error() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .import_file(
                String::from("/nonexistent/livtet-mcp-probe.epub"),
                String::from("link"),
            )
            .await
            .expect_err("there is no such file");
        assert_eq!(error.code, "import");
    }

    #[tokio::test]
    async fn an_empty_batch_answers_with_no_files() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let result = bridge
            .import_files(Vec::new(), String::from("copy"))
            .await
            .expect("an empty batch is not an error");
        assert_eq!(result["files"], json!([]));
        assert_eq!(result["imported"], json!(0));
        assert_eq!(result["failed"], json!(0));
    }

    #[tokio::test]
    async fn reader_resource_for_an_unknown_edition_is_a_reader_error() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .reader_resource(DbId::new().to_string(), String::from("OEBPS/ch1.xhtml"))
            .await
            .expect_err("nothing to read");
        assert_eq!(error.code, "reader");
    }

    #[tokio::test]
    async fn opds_server_status_reports_the_stored_settings() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let status = bridge
            .opds_server_status()
            .await
            .expect("status is readable");

        assert_eq!(status["settings"]["enabled"], json!(false));
        assert_eq!(status["settings"]["port"], json!(8080));
        assert_eq!(status["running"], json!(false));
        assert_eq!(status["url"], json!(null));
    }

    #[tokio::test]
    async fn opds_server_settings_save_refuses_port_zero_under_its_own_code() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .opds_server_settings_save(json!({ "enabled": true, "port": 0 }))
            .await
            .expect_err("port 0 is refused");

        assert_eq!(error.code, "invalid-port");
        assert!(
            !bridge
                .opds_server_status()
                .await
                .expect("status")
                .pointer("/settings/enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            "a refused save leaves the stored settings alone"
        );
    }

    #[tokio::test]
    async fn opds_server_settings_save_persists_a_disabled_configuration() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let saved = bridge
            .opds_server_settings_save(json!({ "enabled": false, "port": 9100 }))
            .await
            .expect("a disabled save binds nothing");
        assert_eq!(saved["settings"]["port"], json!(9100));
        assert_eq!(saved["running"], json!(false));

        let status = bridge.opds_server_status().await.expect("status");
        assert_eq!(status["settings"]["port"], json!(9100));
    }

    #[tokio::test]
    async fn opds_server_start_without_an_index_reports_the_index_code() {
        let (_db, bridge, _edition) = seeded_catalog().await;

        let error = bridge
            .opds_server_start()
            .await
            .expect_err("no index, no listener");

        assert_eq!(error.code, "index");
    }
}
