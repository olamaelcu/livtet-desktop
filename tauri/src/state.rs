//! Application state shared across Tauri commands.
//!
//! [`AppDirectories`] is the read-only filesystem layout the app needs:
//! a SQLite database path, a logs directory, and a Tantivy search
//! index path. [`AppState`] is the live, in-process state — a
//! [`SharedState`] for DB access and an `Arc<SearchIndex>` for search.
//! Commands take this through `tauri::State<'_, AppState>`.

use std::sync::Arc;

use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tauri::{App, Manager};

use crate::Error;

#[derive(Debug)]
pub struct AppDirectories {
    pub database_path: Utf8PathBuf,
    pub logs_dir: Utf8PathBuf,
    /// Parent directory that will contain the tantivy index.
    pub search_index_path: Utf8PathBuf,
}

impl AppDirectories {
    #[tracing::instrument(skip(app), ret, err)]
    pub async fn resolve(app: &App) -> miette::Result<Self> {
        let path_resolver = app.path();
        let local_data_dir_path =
            Utf8PathBuf::from_path_buf(path_resolver.app_local_data_dir().into_diagnostic()?)
                .map_err(Error::PathResolution)?;
        let cache_dir_path =
            Utf8PathBuf::from_path_buf(path_resolver.app_cache_dir().into_diagnostic()?)
                .map_err(Error::PathResolution)?;

        let data_dir_path_exists =
            fs_err::tokio::try_exists(&local_data_dir_path).await.ok() == Some(true);
        if !data_dir_path_exists {
            fs_err::tokio::create_dir_all(&local_data_dir_path)
                .await
                .into_diagnostic()?;
        }

        let logs_dir = local_data_dir_path.join("logs");
        let database_path = local_data_dir_path.join("livtet.sqlite");
        let search_index_path = cache_dir_path.join("search_index");

        Ok(Self {
            database_path,
            logs_dir,
            search_index_path,
        })
    }
}

/// The state managed into Tauri's app handle during `setup`. Commands
/// receive it via `tauri::State<'_, AppState>`.
///
/// `livtet_core::search` is a re-export of the `livtet-search` crate
/// (see `livtet_core::lib`). We route through `livtet_core` so we
/// don't need to add `livtet-search` as a second direct git dep —
/// `livtet-core` (also a git dep) already pulls it transitively.
pub struct AppState {
    pub db: livtet_core::data::SharedState,
    pub search: Arc<livtet_core::search::SearchIndex>,
    pub http: reqwest::Client,
    pub search_registry: crate::commands::remote_search::chain::SearchRegistry,
}
