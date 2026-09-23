use std::sync::Arc;
use tokio::sync::RwLock;

use camino::Utf8PathBuf;
use livtet_core::data::SharedState;
use livtet_core::search::SearchIndex;

pub type ArcMut<T> = Arc<RwLock<T>>;

#[derive(Clone)]
pub struct AppState {
    pub search_index: ArcMut<Option<SearchIndex>>,
    /// Shared database pool, set during app setup.
    pub db: SharedState,
    /// Where extracted cover images are written (`<data>/covers`).
    pub covers_dir: Utf8PathBuf,
    /// Path to the plugin host binary.
    pub plugin_host_path: Utf8PathBuf,
    /// Path to the host.toml configuration.
    pub plugin_host_config: Utf8PathBuf,
    /// Path to the plugins directory.
    pub plugins_dir: Utf8PathBuf,
    /// Bridge to the out-of-process sync daemon.
    pub sync: Arc<crate::sync::SyncHandle>,
}
