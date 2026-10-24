//! Fixture-only helpers: `unwrap`/`expect` document impossible states here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

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

/// In-memory [`CatalogStore`] double for the fixture.
#[derive(Default)]
struct MemoryCatalogs {
    value: Mutex<Option<serde_json::Value>>,
}

impl crate::commands::opds::CatalogStore for MemoryCatalogs {
    fn read(&self) -> Option<serde_json::Value> {
        self.value.lock().unwrap().clone()
    }

    fn write(&self, value: serde_json::Value) -> Result<(), crate::error::OpdsError> {
        *self.value.lock().unwrap() = Some(value);
        Ok(())
    }
}

pub async fn mock_state_for_bulk(test_db: &TestDb) -> AppState {
    let db = test_db.state().clone();
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
        // Real handle over an in-memory duplex; the bulk wrappers never
        // talk to the daemon.
        sync: Arc::new(crate::sync::fixture_handle()),
        opds_http: reqwest::Client::new(),
        atproto: std::sync::Arc::new(crate::atproto::AtprotoManager::new(reqwest::Client::new())),
        // The catalog store seam: bulk wrappers never read this field.
        opds_store: Arc::new(MemoryCatalogs::default()),
        secrets: Arc::new(crate::secrets::KeyringSecretStore),
        // LoopbackServer fields are public; construct directly.
        loopback: crate::commands::byte_server::LoopbackServer {
            base_url: "http://127.0.0.1:0".into(),
            token: "mock".into(),
        },
        readers: Arc::new(Mutex::new(HashMap::new())),
    }
}
