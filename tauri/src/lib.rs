use std::sync::{Arc, Mutex};

use camino::{Utf8Path, Utf8PathBuf};
use livtet_plugins::host_manager::{CommandEnv, HostSpawnConfig, PluginHostManager};
use livtet_plugins::keys::TrustStore;
use livtet_plugins::repository::hmac::HmacKey;
use miette::IntoDiagnostic;
use tauri::{App, Emitter, Manager};
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_forest::{ForestLayer, traits::*, util::EnvFilter};
use tracing_subscriber::fmt;

mod commands;
pub mod cover_storage;
pub mod http;
mod logging_rate_limit;
pub mod secrets;
pub mod state;

#[doc(hidden)]
pub mod _bindings_export {
    //! Public re-exports for the `generate-bindings` bin.
    use crate::commands;
    pub use crate::commands::covers;
    pub use crate::commands::diagnostics;
    pub use crate::commands::digital_inventory;
    pub use crate::commands::edition;
    pub use crate::commands::fonts;
    pub use crate::commands::edition_authors;
    pub use crate::commands::edition_identifiers;
    pub use crate::commands::import_edition;
    pub use crate::commands::keyring;
    pub use crate::commands::language_preference;
    pub use crate::commands::plugins;
    pub use crate::commands::reindex;
    pub use crate::commands::remote_search;
    pub use crate::commands::search;
    pub use crate::commands::window;
    pub use crate::state;

    pub fn specta() -> tauri_specta::Builder<tauri::Wry> {
        tauri_specta::Builder::<tauri::Wry>::new()
            .commands(tauri_specta::collect_commands![
                commands::window::sync_window_title,
                commands::search::search,
                commands::edition::find_edition_by_id,
                commands::edition::find_edition_by_identifier,
                commands::digital_inventory::find_files_by_edition,
                commands::digital_inventory::add_digital_inventory,
                commands::digital_inventory::remove_book,
                commands::edition_authors::find_authors_by_edition,
                commands::edition_identifiers::find_identifiers_by_edition,
                commands::remote_search::remote_search,
                commands::remote_search::cancel_remote_search,
                commands::keyring::get_hardcover_key,
                commands::keyring::set_hardcover_key,
                commands::keyring::clear_hardcover_key,
                commands::keyring::verify_hardcover_key,
                commands::language_preference::get_language_preference,
                commands::language_preference::set_language_preference,
                commands::import_edition::import_edition,
                commands::diagnostics::export_logs,
                commands::reindex::reindex,
                commands::covers::fetch_cover,
                commands::covers::list_covers,
                commands::fonts::download_font,
                commands::fonts::delete_font,
                commands::fonts::list_downloaded_fonts,
                commands::plugins::list_plugins,
                commands::plugins::load_plugin,
                commands::plugins::unload_plugin,
                commands::plugins::set_plugin_disabled,
                commands::plugins::call_plugin_capability,
                commands::plugins::get_plugin_setting,
                commands::plugins::write_plugin_setting,
            ])
            .events(tauri_specta::collect_events![
                crate::commands::remote_search::chain::ProviderFailureEvent,
                crate::commands::reindex::ReindexComplete,
            ])
    }
}

pub use _bindings_export::specta;

use state::{AppDirectories, AppState};

use crate::cover_storage::CacacheStorage;

#[derive(thiserror::Error, miette::Diagnostic, Debug)]
pub enum Error {
    #[error("Could not obtain the UTF-8 version of this path {0:?}")]
    PathResolution(std::path::PathBuf),
}

static LOG_FILE_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

#[cfg(test)]
fn init_test_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(ForestLayer::default())
        .init();
}

async fn init_tracing(
    logs_dir: &Utf8Path,
    subsystem_loggers: &[SubsystemLogger],
) -> miette::Result<()> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        if let Ok(mut guard) = LOG_FILE_GUARD.lock() {
            drop(guard.take());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        prev_hook(info);
    }));

    fs_err::tokio::create_dir_all(&logs_dir)
        .await
        .into_diagnostic()?;
    let env_filter = EnvFilter::try_from_default_env().into_diagnostic()?;

    let filter = env_filter
        .add_directive("tokio_tungstenite=off".parse().into_diagnostic()?)
        .add_directive("tokio_tungstenite::compat=off".parse().into_diagnostic()?);

    let rate_limit = logging_rate_limit::from_env();

    let file_appender = RollingFileAppender::new(Rotation::DAILY, logs_dir, "livtet.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let non_blocking_writer = file_writer;
    *LOG_FILE_GUARD.lock().unwrap() = Some(guard);

    let use_json = std::env::var("LIVTET_LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    if use_json {
        tracing_subscriber::registry()
            .with(filter)
            .with(rate_limit)
            .with(ForestLayer::default())
            .with(
                fmt::layer()
                    .json()
                    .with_writer(non_blocking_writer)
                    .with_target(true)
                    .with_file(true)
                    .with_line_number(true),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(rate_limit)
            .with(ForestLayer::default())
            .with(
                fmt::layer()
                    .with_writer(non_blocking_writer)
                    .with_ansi(false)
                    .with_target(true)
                    .with_file(true)
                    .with_line_number(true),
            )
            .init();
    }

    for _ss in subsystem_loggers {
        // Subsystem loggers are registered by each subsystem during
        // its own init phase via the `SubsystemLogger` type.  The
        // loop here is intentionally empty — the struct exists so
        // future callers have a typed contract.
        let _ = _ss;
    }

    Ok(())
}

#[derive(Clone)]
pub struct SubsystemLogger {
    pub name: String,
    pub filter: EnvFilter,
}

#[tracing::instrument(err)]
async fn setup_database(
    database_path: &Utf8Path,
) -> miette::Result<livtet_core::data::SharedState> {
    if fs_err::tokio::try_exists(&database_path).await.ok() != Some(true) {
        fs_err::tokio::write(&database_path, &[])
            .await
            .into_diagnostic()?;
    }

    let db = livtet_core::data::SharedState::connect(database_path.as_str())
        .await
        .into_diagnostic()?;

    Ok(db)
}

struct TauriEventEmitter {
    app_handle: tauri::AppHandle,
}

impl livtet_plugins::host_manager::EventEmitter for TauriEventEmitter {
    fn emit(&self, name: &str, payload: serde_json::Value) {
        let _ = self.app_handle.emit(name, payload);
    }
}

#[tracing::instrument(skip(app), ret, err)]
async fn app_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    let paths = AppDirectories::resolve(app).await?;
    init_tracing(&paths.logs_dir, &[]).await?;
    tracing::trace!(
        db_path = paths.database_path.to_string(),
        logs_dir = paths.logs_dir.to_string(),
        search_index_path = paths.search_index_path.to_string(),
        "Connecting to the database and opening search index..."
    );

    let db = setup_database(&paths.database_path).await?;
    // If the index directory was created by a previous `SearchIndex::open`
    // without ever being populated via `migrate_to`, the version sidecar
    // will say v2 but the index is empty.  Remove the sidecar so the
    // `migrate_to` guard sees a missing version (→ prev=1) and triggers a
    // full rebuild from the database.
    let sidecar = paths.search_index_path.join("search_schema_version.json");
    if fs_err::tokio::try_exists(&sidecar).await.ok() == Some(true) {
        let empty = livtet_core::search::SearchIndex::open(paths.search_index_path.as_path())
            .map(|ix| {
                let searcher = ix.index().reader().unwrap().searcher();
                searcher.num_docs() == 0
            })
            .unwrap_or(true);
        if empty {
            tracing::info!(
                "search index exists but has zero documents; removing sidecar to force rebuild"
            );
            fs_err::tokio::remove_file(&sidecar)
                .await
                .into_diagnostic()?;
        }
    }
    livtet_core::search::SearchIndex::migrate_to(paths.search_index_path.as_path(), &db.db_conn())
        .await?;
    let search_index = livtet_core::search::SearchIndex::open(paths.search_index_path.as_path())?;
    let search = Arc::new(RwLock::new(search_index));
    let http = crate::http::build_client();
    let search_registry = crate::commands::remote_search::chain::SearchRegistry::default();
    let covers = Arc::new(tokio::sync::Mutex::new(CacacheStorage::new(
        paths.covers_cache_dir.clone(),
        paths.covers_permanent_dir.clone(),
    )));

    // ── Plugin host ──────────────────────────────────────────
    let hmac_key = Arc::new(HmacKey::load_or_create_in_keyring()
        .map_err(|e| format!("failed to load HMAC key: {e}"))?);

    let host_binary = std::env::current_exe().into_diagnostic()?;
    let host_binary_path = Utf8PathBuf::from_path_buf(
        host_binary
            .parent()
            .ok_or_else(|| "no parent dir for executable")?
            .join("livtet-plugins-host-lua"),
    )
    .map_err(|p| format!("non-utf8 path: {p:?}"))?;

    let plugin_dir = paths.plugins_dir.clone();
    fs_err::tokio::create_dir_all(plugin_dir.as_std_path())
        .await
        .into_diagnostic()?;

    let command_env = CommandEnv {
        lua_path: None,
        lua_cpath: None,
    };
    let spawn_config = HostSpawnConfig { command_env };

    let mut host = PluginHostManager::spawn_with_db_emitter_log_dir(
        &host_binary_path,
        plugin_dir.clone(),
        Some(db.pool.clone()),
        Some(paths.logs_dir.clone()),
        hmac_key,
        spawn_config,
    )
    .await
    .map_err(|e| format!("failed to spawn plugin host: {e}"))?;

    let trust_store = TrustStore::load()
        .map_err(|e| format!("failed to load trust store: {e}"))?;
    host.with_trust_store(trust_store);

    let app_handle = app.handle().clone();
    let emitter = Arc::new(TauriEventEmitter { app_handle });
    host.with_event_emitter(emitter);

    let plugin_host = Arc::new(TokioMutex::new(host));

    app.manage(AppState {
        db,
        search,
        http,
        search_registry,
        covers,
        logs_dir: paths.logs_dir.clone(),
        search_index_path: paths.search_index_path.clone(),
        fonts_dir: paths.fonts_dir.clone(),
        plugin_host,
        plugin_dir,
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_store::Builder::default().build());

    let specta_builder = specta();

    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    #[cfg(feature = "e2e-testing")]
    let builder = builder.plugin(tauri_plugin_playwright::init());

    builder
        .invoke_handler(specta_builder.invoke_handler())
        .setup(move |app| {
            use tauri_plugin_decorum::WebviewWindowExt;

            if let Some(window) = app.get_webview_window("main") {
                window.create_overlay_titlebar()?;

                #[cfg(target_os = "macos")]
                {
                    window.set_traffic_lights_inset(12.0, 16.0)?;
                    window.make_transparent()?;
                }
            }
            tauri::async_runtime::block_on(app_setup(app))?;
            specta_builder.mount_events(app);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
