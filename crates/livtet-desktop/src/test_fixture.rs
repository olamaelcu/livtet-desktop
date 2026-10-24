//! Minimal `AppState` fixture using real types from the repo.
//! Only fields read by the 5 bulk wrappers (`db`, `search_index`) are
//! real; `opds_store` needs a `tauri_plugin_store::Store` backed by the
//! mock runtime (see note below). Build with `tauri::test::mock_app()`.
//!
//! Usage (after enabling `tauri/test`):
//! ```rust,ignore
//! let db = test_db.state().db_conn();
//! let state = mock_state_for_bulk(&test_db).await;
//! let app = tauri::test::mock_app();
//! app.manage(state.clone());
//! ```

use crate::types::{AppState, ArcMut};
use camino::Utf8PathBuf;
use livtet_core::data::TestDb;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub async fn mock_state_for_bulk(test_db: &TestDb) -> AppState {
    let db = test_db.state().db_conn();
    AppState {
        search_index: ArcMut::new(tokio::sync::RwLock::new(None)),
        db,
        covers_dir: Utf8PathBuf::from_path_buf(std::env::temp_dir().join("mock_covers"))
            .expect("utf8"),
        books_dir: Utf8PathBuf::from_path_buf(std::env::temp_dir().join("mock_books"))
            .expect("utf8"),
        plugin_host_path: Utf8PathBuf::from("/dev/null"),
        plugin_host_config: Utf8PathBuf::from("/dev/null"),
        plugins_dir: Utf8PathBuf::from("/dev/null/plugins"),
        disabled_plugins_dir: Utf8PathBuf::from("/dev/null/disabled"),
        // SyncHandle: reuse the crate's own test `connect()` (a duplex
        // stream), so the field is a real handle, not a stub.
        sync: Arc::new(crate::sync::connect(std::time::Duration::from_secs(1))),
        opds_http: reqwest::Client::new(),
        atproto: std::sync::Arc::new(crate::atproto::AtprotoManager::new(reqwest::Client::new())),
        // `tauri_plugin_store::Store` resolves its path relative to the
        // app data dir, but the path-only constructor works without a
        // handle. Bulk wrappers never read this field; OPDS tests can
        // rebuild it from the mock-app handle instead.
        opds_store: Arc::new(tauri_plugin_store::Store::from("mock_opds.json")),
        secrets: Arc::new(crate::secrets::KeyringSecretStore),
        atproto: Arc::new(crate::atproto::AtprotoManager::new(reqwest::Client::new())),
        // LoopbackServer fields are public; construct directly.
        loopback: crate::commands::byte_server::LoopbackServer {
            base_url: "http://127.0.0.1:0".into(),
            token: "mock".into(),
        },
        readers: Arc::new(Mutex::new(HashMap::new())),
    }
}
