pub mod commands;
mod error;
mod roles;
mod secrets;
pub mod sync;
mod types;

pub use error::{PluginError, SearchIndexError};
pub use types::{AppState, ArcMut};

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tauri::{App, Manager};
use tauri_specta::{Builder, collect_commands};
use tokio::sync::RwLock;
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_forest::{
    ForestLayer, Formatter, Processor, printer::Pretty, processor, traits::*, tree::Tree,
    util::EnvFilter,
};
use tracing_subscriber::fmt;

#[derive(thiserror::Error, miette::Diagnostic, Debug)]
pub enum Error {
    #[error("Could not obtain the UTF-8 version of this path {0:?}")]
    PathResolution(std::path::PathBuf),
}

static LOG_FILE_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

#[allow(clippy::result_large_err)]
fn write_pretty(tree: Tree) -> processor::Result {
    if let Ok(line) = Pretty.fmt(&tree) {
        let _ = std::io::stdout().write_all(line.as_bytes());
    }
    Ok(())
}

fn stdout_processor() -> impl Processor {
    processor::from_fn(write_pretty)
}

async fn init_tracing(logs_dir: Utf8PathBuf) -> miette::Result<()> {
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if IN_PANIC_HOOK.with(|flag| flag.replace(true)) {
            return;
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracing::error!("panic: {info}");
        }));
        if let Ok(mut guard) = LOG_FILE_GUARD.lock() {
            drop(guard.take());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        prev_hook(info);
        IN_PANIC_HOOK.with(|flag| flag.set(false));
    }));

    fs_err::tokio::create_dir_all(&logs_dir)
        .await
        .into_diagnostic()?;
    let env_filter = EnvFilter::try_from_default_env().into_diagnostic()?;

    let filter = env_filter
        .add_directive("tokio_tungstenite=warn".parse().into_diagnostic()?)
        .add_directive("tokio_tungstenite::compat=warn".parse().into_diagnostic()?);

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
            .with(ForestLayer::from(stdout_processor()))
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
            .with(ForestLayer::from(stdout_processor()))
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

    Ok(())
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub database_path: Utf8PathBuf,
    pub logs_dir: Utf8PathBuf,
    pub search_index_path: Utf8PathBuf,
    pub covers_dir: Utf8PathBuf,
    pub books_dir: Utf8PathBuf,
    pub plugins_dir: Utf8PathBuf,
}

impl Paths {
    pub fn new(app_dir: &Utf8PathBuf) -> Self {
        let data_dir = app_dir.join("data");
        let search_index = app_dir.join("search");
        let logs = app_dir.join("logs");
        let plugins_dir = app_dir.join("plugins");

        Self {
            covers_dir: data_dir.join("covers"),
            books_dir: data_dir.join("books"),
            database_path: data_dir,
            logs_dir: logs,
            search_index_path: search_index,
            plugins_dir,
        }
    }
}

#[tracing::instrument(skip_all, level = "info")]
async fn setup_database(
    database_path: Utf8PathBuf,
) -> miette::Result<livtet_core::data::SharedState> {
    if fs_err::tokio::try_exists(&database_path).await.ok() != Some(true) {
        fs_err::tokio::create_dir_all(&database_path)
            .await
            .into_diagnostic()?;
    }

    tracing::trace!(db_path = %database_path.join("livtet.db"));
    let db = livtet_core::data::SharedState::connect(
        database_path.join("livtet.db").as_str(),
        &[livtet_core::data::Kind::Business],
    )
    .await
    .into_diagnostic()?;

    Ok(db)
}

const PLUGIN_HOST_BIN: &str = if cfg!(windows) {
    "livtet-plugin-host.exe"
} else {
    "livtet-plugin-host"
};

/// Resolves the plugin host binary: an explicit override first, then next to the
/// application binary (dev and bundled installs), then the application data dir.
fn resolve_plugin_host(app_dir: &Utf8PathBuf) -> Utf8PathBuf {
    if let Some(path) = std::env::var_os("LIVTET_PLUGIN_HOST") {
        return Utf8PathBuf::from(path.to_string_lossy().into_owned());
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(PLUGIN_HOST_BIN);
        if candidate.is_file() {
            return Utf8PathBuf::from_path_buf(candidate)
                .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()));
        }
    }

    app_dir.join(PLUGIN_HOST_BIN)
}

#[tracing::instrument(err, skip_all, level = "info")]
async fn setup_plugin_host(
    app_dir: &Utf8PathBuf,
    plugins_dir: &Utf8PathBuf,
) -> miette::Result<(Utf8PathBuf, Utf8PathBuf, Utf8PathBuf)> {
    fs_err::tokio::create_dir_all(plugins_dir)
        .await
        .into_diagnostic()?;

    let host_config = app_dir.join("host.toml");
    if !host_config.exists() {
        let config = "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"fs_read\"]\n\n[signatures]\nrequired = false\n";
        std::fs::write(&host_config, config).into_diagnostic()?;
    }

    let plugin_host_path = resolve_plugin_host(app_dir);
    if !plugin_host_path.is_file() {
        tracing::warn!(
            path = %plugin_host_path,
            "plugin host binary not found; plugin commands will fail until it is available"
        );
    }

    Ok((plugin_host_path, host_config, plugins_dir.clone()))
}

#[tracing::instrument(skip_all, err, level = "info")]
async fn app_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    let data_dir = livtet_core::paths::data_dir().unwrap_or_else(|| {
        let current_dir = std::env::current_dir().expect("current dir");
        Utf8PathBuf::from_path_buf(current_dir).expect("valid utf8 path")
    });

    let paths = Paths::new(&data_dir);

    init_tracing(paths.logs_dir.clone()).await?;
    tracing::trace!(
        db_path = paths.database_path.to_string(),
        logs_dir = paths.logs_dir.to_string(),
        search_index_path = paths.search_index_path.to_string(),
        "Connecting to the database and opening search index..."
    );

    let db = setup_database(paths.database_path.clone()).await?;

    if fs_err::tokio::try_exists(&paths.search_index_path)
        .await
        .ok()
        != Some(true)
    {
        fs_err::tokio::create_dir_all(&paths.search_index_path)
            .await
            .into_diagnostic()?;
    }

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

    fs_err::tokio::create_dir_all(&paths.covers_dir)
        .await
        .into_diagnostic()?;

    fs_err::tokio::create_dir_all(&paths.books_dir)
        .await
        .into_diagnostic()?;

    let (plugin_host_path, plugin_host_config, plugins_dir) =
        setup_plugin_host(&data_dir, &paths.plugins_dir)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
            })?;

    // The sync daemon owns the same SQLite file as the desktop app; it opens
    // its own connection pool and runs the client-side migrations.
    let sync_db_path = paths.database_path.join("livtet.db");
    let sync = crate::sync::SyncHandle::spawn(app.handle(), &sync_db_path)?;

    // Shared HTTP client for OPDS catalog requests: consistent User-Agent and a
    // bounded timeout. All remote fetches stay in Rust (CSP keeps `connect-src`
    // at 'self').
    let opds_http = reqwest::Client::builder()
        .user_agent(concat!("livtet-desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    let opds_store = tauri_plugin_store::StoreBuilder::new(&*app, "opds-catalogs.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;

    // Loopback audio server for `<audio>` playback: WebKitGTK routes media
    // through GStreamer, which cannot fetch the app's custom schemes.
    let audio = commands::audio_server::spawn(db.db_conn(), paths.books_dir.clone())
        .await
        .map_err(|e| {
            Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
        })?;

    let state = AppState {
        search_index: ArcMut::new(RwLock::new(None)),
        db,
        covers_dir: paths.covers_dir.clone(),
        books_dir: paths.books_dir.clone(),
        audio,
        plugin_host_path,
        plugin_host_config,
        plugins_dir,
        sync: Arc::new(sync),
        opds_http,
        opds_store,
        secrets: Arc::new(secrets::KeyringSecretStore),
        readers: Arc::new(Mutex::new(HashMap::new())),
    };
    {
        let mut guard = state.search_index.write().await;
        *guard = Some(search_index);
    }

    app.manage(state);

    Ok(())
}

/// Empty `reader` protocol response for a status with no body (400/404/500).
fn reader_status(status: tauri::http::StatusCode) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("reader status is a valid response")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta_builder = Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::catalog::get_edition_detail,
        commands::catalog::get_edition_covers,
        commands::search::search_editions,
        commands::search::search_typeahead,
        commands::search::filter_options,
        commands::bulk::delete_editions,
        commands::bulk::export_editions_csv,
        commands::bulk::add_edition_tags,
        commands::bulk::remove_edition_tags,
        commands::bulk::matching_edition_ids,
        commands::import::import_file,
        commands::import::import_files,
        commands::import::relink_edition_file,
        commands::plugins::list_plugins,
        commands::opds::opds_default_catalogs,
        commands::opds::opds_catalogs_list,
        commands::opds::opds_catalogs_create,
        commands::opds::opds_catalogs_update,
        commands::opds::opds_catalogs_remove,
        commands::opds::opds_catalogs_test,
        commands::opds::opds_feed,
        commands::opds::opds_page,
        commands::opds::opds_search,
        commands::opds::opds_acquire,
        commands::reader::open_reader,
        commands::reader::reader_publication,
        commands::reader::reader_resource,
        commands::sync::sync_health,
        commands::sync::sync_status,
        commands::sync::sync_requests_recent,
        commands::sync::sync_pairing_begin,
        commands::sync::sync_pairing_list,
        commands::sync::sync_pairing_approve,
        commands::sync::sync_pairing_reject,
        commands::sync::sync_devices_list,
        commands::sync::sync_devices_revoke,
        commands::sync::sync_conflicts_list,
        commands::sync::sync_conflicts_resolve,
        commands::sync::sync_server_start,
        commands::sync::sync_server_stop,
        commands::reader::open_reader,
        commands::reader::reader_publication,
        commands::playback::get_listening_progress,
        commands::playback::save_listening_progress,
    ]);

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_decorum::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(specta_builder.invoke_handler())
        .register_asynchronous_uri_scheme_protocol("reader", |ctx, request, responder| {
            let path = request.uri().path().to_string();
            let Some((edition_id, href)) = crate::commands::reader::parse_reader_uri(&path) else {
                responder.respond(reader_status(tauri::http::StatusCode::BAD_REQUEST));
                return;
            };
            let Ok(id) = edition_id.parse::<livtet_types::DbId>() else {
                responder.respond(reader_status(tauri::http::StatusCode::NOT_FOUND));
                return;
            };
            // The cache is populated by `open_reader`/`reader_publication`
            // before any window can request it; serve cached books only.
            let reader = ctx
                .app_handle()
                .try_state::<AppState>()
                .and_then(|state| crate::commands::reader::cached_lookup(&state, &id));
            let Some(reader) = reader else {
                responder.respond(reader_status(tauri::http::StatusCode::NOT_FOUND));
                return;
            };
            // The href goes in exactly as received: decoding and validation
            // happen in `Reader::read`.
            match reader.read(href) {
                Some((media_type, bytes)) => responder.respond(
                    tauri::http::Response::builder()
                        .status(tauri::http::StatusCode::OK)
                        .header("Content-Type", media_type)
                        .header("Access-Control-Allow-Origin", "*")
                        .header("Cache-Control", "no-store")
                        .body(bytes)
                        .unwrap_or_else(|_| {
                            reader_status(tauri::http::StatusCode::INTERNAL_SERVER_ERROR)
                        }),
                ),
                None => responder.respond(reader_status(tauri::http::StatusCode::NOT_FOUND)),
            }
        })
        .on_window_event(|window, event| {
            if !matches!(event, tauri::WindowEvent::Destroyed) {
                return;
            }
            let Some(edition_id) = window
                .label()
                .strip_prefix(crate::commands::reader::READER_WINDOW_PREFIX)
            else {
                return;
            };
            let Ok(id) = edition_id.parse::<livtet_types::DbId>() else {
                return;
            };
            if let Some(state) = window.try_state::<AppState>()
                && let Ok(mut readers) = state.readers.lock()
            {
                readers.remove(&id);
            }
        });

    #[cfg(debug_assertions)]
    {
        specta_builder
            .export(
                specta_typescript::Typescript::default(),
                "../../web/lib/bindings.ts",
            )
            .expect("export bindings");
    }

    #[cfg(feature = "e2e-testing")]
    let builder = builder.plugin(tauri_plugin_playwright::init());

    // Expose the dev automation bridge (local WebSocket) only in debug builds;
    // it grants arbitrary JS/IPC control and must never ship in release.
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    builder
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

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
