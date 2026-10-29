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
    ImportContext, ImportMode, VirtualBookInput, create_virtual_book_and_index, import_batch,
    import_one_inner,
};
use crate::commands::reader::{
    ReaderCache, delete_reader_bookmark, fetch_reader_bookmarks, fetch_reader_position,
    publication_for_edition, resource_text_for_edition, store_reader_bookmark,
    store_reader_position,
};
use crate::commands::search::{
    EditionFilters, filter_options_inner, search_editions_inner, search_typeahead_inner,
};
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
    search_index: ArcMut<Option<SearchIndex>>,
    covers_dir: Utf8PathBuf,
    books_dir: Utf8PathBuf,
    plugin_host_path: Utf8PathBuf,
    plugin_host_config: Utf8PathBuf,
    plugins_dir: Utf8PathBuf,
    readers: Arc<ReaderCache>,
    loopback: crate::commands::byte_server::LoopbackServer,
}

impl CommandBridge {
    /// Build the bridge for a running app state.
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self {
            db: state.db.clone(),
            search_index: state.search_index.clone(),
            covers_dir: state.covers_dir.clone(),
            books_dir: state.books_dir.clone(),
            plugin_host_path: state.plugin_host_path.clone(),
            plugin_host_config: state.plugin_host_config.clone(),
            plugins_dir: state.plugins_dir.clone(),
            readers: state.readers.clone(),
            loopback: state.loopback.clone(),
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
        let hits = search_typeahead_inner(index.as_ref(), &query, limit)
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
            index.as_ref(),
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
        let outcome = create_virtual_book_and_index(&self.db, &self.search_index, input)
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

    use livtet_core::data::{Kind, TestDb};
    use serde_json::json;

    use super::*;
    use crate::test_fixture::MemoryMcpSettings;

    /// A runtime over an in-memory store plus a bridge over the fixture
    /// `AppState`: real command code, in-memory database, no search index
    /// loaded (the fixture never opens one).
    async fn harness() -> (McpRuntime, CommandBridge) {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = crate::mock_state_for_bulk(&test_db).await;
        let runtime = McpRuntime::new(Arc::new(MemoryMcpSettings::default()));
        (runtime, CommandBridge::from_state(&state))
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
        let (runtime, bridge) = harness().await;
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
        let (runtime, bridge) = harness().await;

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
        let (runtime, bridge) = harness().await;
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
        let (_runtime, bridge) = harness().await;
        let hits = bridge
            .search_typeahead(String::new(), None)
            .await
            .expect("blank query is never an error");
        assert_eq!(hits, json!([]));
    }

    #[tokio::test]
    async fn typeahead_maps_an_absent_index_to_a_search_error() {
        let (_runtime, bridge) = harness().await;
        let error = bridge
            .search_typeahead(String::from("dune"), None)
            .await
            .expect_err("no index is loaded");
        assert_eq!(error.code, "search");
        assert!(error.message.contains("Unavailable"), "{error:?}");
    }

    #[tokio::test]
    async fn filter_options_reads_every_axis_from_the_catalog() {
        let (_runtime, bridge) = harness().await;
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
        let (_runtime, bridge) = harness().await;
        let error = bridge
            .get_reader_position(String::from("not-an-id"))
            .await
            .expect_err("unparseable id");
        assert_eq!(error.code, "invalid-id");
    }

    #[tokio::test]
    async fn unknown_import_modes_are_rejected() {
        let (_runtime, bridge) = harness().await;
        let error = bridge
            .import_file(String::from("/tmp/book.epub"), String::from("sideways"))
            .await
            .expect_err("unknown mode");
        assert_eq!(error.code, "invalid-parameter");
    }

    #[tokio::test]
    async fn an_unknown_edition_describes_as_no_publication() {
        let (_runtime, bridge) = harness().await;
        let publication = bridge
            .reader_publication(DbId::new().to_string())
            .await
            .expect("an unknown edition is not an error");
        assert_eq!(publication, Value::Null);
    }
}
