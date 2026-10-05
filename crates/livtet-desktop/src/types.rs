use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use camino::Utf8PathBuf;
use livtet_core::data::SharedState;
use livtet_core::search::SearchIndex;
use livtet_types::DbId;

pub type ArcMut<T> = Arc<RwLock<T>>;

#[derive(Clone)]
pub struct AppState {
    pub search_index: ArcMut<Option<SearchIndex>>,
    /// Shared database pool, set during app setup.
    pub db: SharedState,
    /// Where extracted cover images are written (`<data>/covers`).
    pub covers_dir: Utf8PathBuf,
    /// Where library-owned book files live (`<data>/books`).
    pub books_dir: Utf8PathBuf,
    /// Path to the plugin host binary.
    pub plugin_host_path: Utf8PathBuf,
    /// Path to the host.toml configuration.
    pub plugin_host_config: Utf8PathBuf,
    /// Path to the plugins directory: the set of directories here is exactly the
    /// set of plugins the host loads.
    pub plugins_dir: Utf8PathBuf,
    /// Sibling root holding disabled plugins. Moving a plugin's directory here is
    /// how it is turned off; moving it back turns it on (ADR-0034).
    pub disabled_plugins_dir: Utf8PathBuf,
    /// Bridge to the out-of-process sync daemon.
    pub sync: Arc<crate::sync::SyncHandle>,
    /// Shared HTTP client for OPDS catalog requests.
    pub opds_http: reqwest::Client,
    /// Rust-owned store of subscribed OPDS catalogs (metadata only).
    pub opds_store: Arc<tauri_plugin_store::Store<tauri::Wry>>,
    /// OS-keyring-backed storage for OPDS catalog credentials.
    pub secrets: Arc<dyn crate::secrets::SecretStore>,
    /// Loopback HTTP server serving library file bytes to the webview.
    pub loopback: crate::commands::byte_server::LoopbackServer,
    /// Open EPUB publications cached by edition id for the reader window.
    ///
    /// `DbId` is `Hash`, so it works as the cache key directly. Entries are
    /// removed when their `reader-{id}` window is destroyed.
    pub readers: Arc<Mutex<HashMap<DbId, Arc<livtet_reader::Reader>>>>,
}
