pub mod atproto;
pub mod commands;
mod error;
pub mod firewall;
mod http;
mod koreader_sync;
mod mcp;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod menu;
mod opds_server;
mod paths;
mod plugin_host;
pub mod roles;
pub mod secrets;
pub mod secrets_embed;
pub mod spotlight;
pub mod sync;
pub mod sync_diagnostics;
pub mod sync_settings;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod tray;
pub mod types;

pub use error::{PluginError, SearchIndexError};
pub use koreader_sync::{KOREADER_DAEMON_FLAG, run_koreader_daemon};
pub use paths::Paths;
pub use types::{AppState, ArcMut};

use std::cell::Cell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;

use crate::http::reader_status;
use crate::plugin_host::resolve_plugin_host;
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
    PathResolution(camino::Utf8PathBuf),
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

/// Crates whose per-packet chatter is not worth a line at the level the
/// environment asks for.
///
/// A dev `RUST_LOG` here ends with a bare `trace`, which sets the default level
/// for every target and so overrides any `iroh=...` a developer writes in front of
/// it. These directives are appended *after* the environment's, and a later
/// `EnvFilter` directive wins — that ordering is what makes them effective
/// rather than merely present. `warn` keeps a real transport failure visible
/// while dropping the frame-by-frame TRACE/DEBUG stream that, measured, was
/// 43,000 of the last 100,000 log lines.
const QUIET_TRANSPORTS: &[&str] = &[
    "iroh=warn",
    "iroh_base=warn",
    "iroh_dns=warn",
    "iroh_mdns_address_lookup=warn",
    "iroh_metrics=warn",
    "iroh_relay=warn",
    "mdns_sd=warn",
    "quinn=warn",
    "quinn_proto=warn",
    "quinn_udp=warn",
    "swarm_discovery=warn",
    "acto=warn",
    // What the transport's own plumbing logs under it: name resolution, the
    // netlink socket it watches interfaces through, and the poll loop.
    "hickory_proto=warn",
    "hickory_resolver=warn",
    "netlink_proto=warn",
    "netlink_sys=warn",
    "netwatch=warn",
    "mio=warn",
    // iroh 1.3's QUIC stack, its TLS, and the DNS resolver it resolves relays
    // through — the crates the first mute list missed.
    "noq=warn",
    "noq_proto=warn",
    "noq_udp=warn",
    "rustls=warn",
    "n0_dns_resolver=warn",
    "hyper_util=warn",
    "reqwest=warn",
    "tokio_tungstenite=warn",
    "tokio_tungstenite::compat=warn",
];

/// Apply [`QUIET_TRANSPORTS`] to a filter, so they win over whatever level it
/// already carries.
fn quiet_transports(mut filter: EnvFilter) -> miette::Result<EnvFilter> {
    for directive in QUIET_TRANSPORTS {
        filter = filter.add_directive(directive.parse().into_diagnostic()?);
    }
    Ok(filter)
}

/// `RUST_LOG`, with the transport mutes appended so they win over it.
fn log_filter() -> miette::Result<EnvFilter> {
    quiet_transports(EnvFilter::try_from_default_env().into_diagnostic()?)
}

#[cfg(test)]
mod logging_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use tracing::Level;
    use tracing_subscriber::layer::SubscriberExt;

    use super::{EnvFilter, quiet_transports};

    /// A dev `RUST_LOG` ends in a bare `trace`, which sets the default level for
    /// every target — so the mutes only work by being appended after it. Both
    /// halves are asserted: a later directive that stopped winning would put the
    /// transport firehose back, and dropping one of the mutes would too.
    #[test]
    fn transport_mutes_override_a_global_trace_setting() {
        let filter = quiet_transports(EnvFilter::new("trace")).unwrap();
        let subscriber = tracing_subscriber::registry().with(filter);
        tracing::subscriber::with_default(subscriber, || {
            // `event_enabled!` needs its target at compile time, so the list is
            // unrolled rather than looped.
            macro_rules! assert_muted {
                ($($target:literal),+ $(,)?) => {
                    $(
                        assert!(
                            !tracing::event_enabled!(target: $target, Level::TRACE),
                            "{} at TRACE must be muted",
                            $target
                        );
                        assert!(
                            tracing::event_enabled!(target: $target, Level::WARN),
                            "{} at WARN must still reach the log",
                            $target
                        );
                    )+
                };
            }

            assert_muted!(
                "iroh",
                "iroh_relay",
                "iroh_mdns_address_lookup",
                "swarm_discovery",
                "acto",
                "quinn_proto",
                "hickory_proto",
                "netlink_proto",
                "netwatch",
                "mio",
                "noq_proto",
                "rustls",
                "n0_dns_resolver",
            );
            assert!(
                tracing::event_enabled!(target: "livtet_desktop", Level::TRACE),
                "the global level must survive for everything else"
            );
        });
    }
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
    let filter = log_filter()?;

    let file_appender = RollingFileAppender::new(Rotation::DAILY, logs_dir, "livtet.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let non_blocking_writer = file_writer;
    *LOG_FILE_GUARD.lock().unwrap_or_else(|poisoned| {
        tracing::error!("log file guard lock poisoned: {poisoned:?}");
        poisoned.into_inner()
    }) = Some(guard);

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

#[tracing::instrument(err, skip_all, level = "info")]
async fn setup_plugin_host(
    app_dir: &Utf8PathBuf,
    plugins_dir: &Utf8PathBuf,
) -> miette::Result<(Utf8PathBuf, Utf8PathBuf, Utf8PathBuf, Utf8PathBuf)> {
    fs_err::tokio::create_dir_all(plugins_dir)
        .await
        .into_diagnostic()?;

    // Disabled plugins are parked in a sibling root the host is never pointed at,
    // which is what turning a plugin off means (ADR-0034).
    let disabled_plugins_dir = plugins_dir
        .parent()
        .unwrap_or(app_dir.as_path())
        .join("plugins-disabled");
    fs_err::tokio::create_dir_all(&disabled_plugins_dir)
        .await
        .into_diagnostic()?;

    let host_config = app_dir.join("host.toml");
    if !host_config.exists() {
        let config = "[capabilities]\nallow = [\"log\", \"xml\", \"sqlite\"]\ncallbacks = [\"fs_read\", \"http\"]\n\n[signatures]\nrequired = false\n";
        fs_err::write(&host_config, config).into_diagnostic()?;
    }

    let plugin_host_path = resolve_plugin_host(app_dir);
    if !plugin_host_path.is_file() {
        tracing::warn!(
            path = %plugin_host_path,
            "plugin host binary not found; plugin commands will fail until it is available"
        );
    }

    Ok((
        plugin_host_path,
        host_config,
        plugins_dir.clone(),
        disabled_plugins_dir,
    ))
}

#[tracing::instrument(skip_all, err, level = "info")]
async fn app_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    let data_dir = livtet_core::paths::data_dir().unwrap_or_else(|| {
        let current_dir = std::env::current_dir().unwrap_or_else(|e| panic!("current dir: {e}"));
        Utf8PathBuf::from_path_buf(current_dir)
            .unwrap_or_else(|e| panic!("valid utf8 path: {}", e.display()))
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
                let searcher = ix
                    .index()
                    .reader()
                    .unwrap_or_else(|e| panic!("search index reader failed: {e}"))
                    .searcher();
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

    let (plugin_host_path, plugin_host_config, plugins_dir, disabled_plugins_dir) =
        setup_plugin_host(&data_dir, &paths.plugins_dir)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
            })?;

    // The sync daemon owns the same SQLite file as the desktop app; it opens
    // its own connection pool and runs the client-side migrations.
    let sync_db_path = paths.database_path.join("livtet.db");
    // Read the chosen name before spawning: it travels as the daemon's
    // `--device-name` flag, so peers see it from the first advertisement.
    let sync_store = tauri_plugin_store::StoreBuilder::new(&*app, "sync.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let sync_store: Arc<dyn sync_settings::SyncSettingsStore> = sync_store;
    let sync_settings = sync_store.read().unwrap_or_default();
    let sync_device_name = sync_settings.device_name;
    // The port is resolved from the store and written back once the daemon is
    // serving. Peers dial the iroh node id, not the port, but a firewall rule
    // names the UDP port, so keeping it stable keeps that rule matching.
    let sync = crate::sync::SyncHandle::spawn_with_remembered_port(
        app.handle(),
        sync_store.as_ref(),
        &sync_db_path,
        sync_device_name.as_deref(),
        &paths.books_dir,
        &paths.covers_dir,
    )
    .await?;

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
    let opds_store: Arc<dyn commands::opds::CatalogStore> = opds_store;

    // The MCP endpoint's settings, kept beside the OPDS catalog store so both
    // live in the app's own store directory (ADR-0041).
    let mcp_store = tauri_plugin_store::StoreBuilder::new(&*app, "mcp.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let mcp_store: Arc<dyn mcp::McpSettingsStore> = mcp_store;

    // The OPDS server's settings, in their own store beside the MCP endpoint's
    // (ADR-0045).
    let opds_server_store = tauri_plugin_store::StoreBuilder::new(&*app, "opds-server.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let opds_server_store: Arc<dyn opds_server::OpdsServerSettingsStore> = opds_server_store;

    // Spotlight's opt-in flag, beside the other desktop settings. The store is
    // opened even on other platforms so the setting survives a platform switch.
    let spotlight_store = tauri_plugin_store::StoreBuilder::new(&*app, "spotlight.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let spotlight_store: Arc<dyn spotlight::SpotlightSettingsStore> = spotlight_store;

    // The KOReader sync server's settings, in their own store beside the OPDS
    // server's. The account's password never lands here: only its MD5, which
    // is all KOReader ever sends.
    let koreader_sync_store = tauri_plugin_store::StoreBuilder::new(&*app, "koreader-sync.json")
        .build()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let koreader_sync_store: Arc<dyn koreader_sync::KoreaderSyncSettingsStore> =
        koreader_sync_store;
    // Loopback byte server for library files the webview must fetch:
    // `<audio>` playback (WebKitGTK routes media through GStreamer, which
    // cannot fetch the app's custom schemes) and ranged PDF reads.
    let loopback = commands::byte_server::spawn(db.db_conn(), paths.books_dir.clone())
        .await
        .map_err(|e| {
            Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error>
        })?;

    let state = AppState {
        search_index: ArcMut::new(RwLock::new(None)),
        db,
        covers_dir: paths.covers_dir.clone(),
        books_dir: paths.books_dir.clone(),
        loopback,
        plugin_host_path,
        plugin_host_config,
        plugins_dir,
        disabled_plugins_dir,
        sync: Arc::new(sync),
        sync_settings: sync_store,
        atproto: Arc::new(crate::atproto::AtprotoManager::new(opds_http.clone())),
        opds_http,
        opds_store,
        secrets: Arc::new(secrets::KeyringSecretStore),
        readers: Arc::new(Mutex::new(HashMap::new())),
        mcp: Arc::new(mcp::McpRuntime::new(mcp_store)),
        opds_server: Arc::new(opds_server::OpdsServerRuntime::new(opds_server_store)),
        spotlight: Arc::new(spotlight::SpotlightRuntime::new(spotlight_store)),
        koreader_sync: Arc::new(koreader_sync::KoreaderSyncRuntime::with_runner(
            koreader_sync_store,
            Arc::new(koreader_sync::KeyringPasswordStore),
            Arc::new(koreader_sync::ProcessRunner::default()),
        )),
        enrichment_cache: ArcMut::new(RwLock::new(HashMap::new())),
    };
    {
        let mut guard = state.search_index.write().await;
        *guard = Some(Arc::new(search_index));
    }

    // The MCP endpoint runs in this process over the same state, so a tool
    // call executes the command code the webview reaches over IPC. It starts
    // only when Settings has it on (or `LIVTET_MCP_PORT` forces it), and a
    // failure is logged rather than failing app setup (ADR-0041).
    let mcp_bridge = mcp::CommandBridge::from_state(&state);
    mcp::start_from_settings(&state.mcp, mcp_bridge).await;

    // The OPDS server publishes the library to reading devices on the network.
    // Like the MCP endpoint it starts only when Settings (or the persisted
    // `opds-server.json`) has it on, and a failure is logged rather than failing
    // app setup: the app works with the server down (ADR-0045).
    let opds_server_deps = opds_server::OpdsServerDeps::from_state(&state);
    opds_server::start_from_settings(&state.opds_server, &opds_server_deps).await;

    // A paired platform's client cannot dial this machine's LAN address, so
    // the sync daemon forwards its OPDS requests here and this process runs
    // them through the same route table the LAN server uses. No port is bound
    // for this path; links the routes build name the LAN, and the dispatch
    // rewrites them to the origin the platform serves them from.
    let link_base = opds_server::link_base_url(&state.opds_server);
    let target = opds_server::platform_origin();
    match opds_server::OpdsDispatch::new(&opds_server_deps, link_base, target).await {
        Ok(dispatch) => state.sync.set_opds_dispatch(dispatch),
        Err(error) => tracing::warn!(
            code = %error.code,
            message = %error.message,
            "OPDS pass-through is unavailable"
        ),
    }

    // A previous session opted in: refresh in the background so editions
    // imported while the app was closed are searchable.
    if state.spotlight.enabled() {
        let spotlight = state.spotlight.clone();
        let db = state.db.db_conn();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = spotlight.reindex(&db).await {
                tracing::warn!(%error, "Spotlight index refresh failed");
            }
        });
    }

    // The KOReader sync server takes pushes from reading devices. It starts on
    // the same terms as the OPDS server — only when the persisted settings say
    // so — and maps the library's documents in the background so the first
    // device request does not pay for a whole library.
    let koreader_sync_deps = koreader_sync::KoreaderSyncDeps::from_state(&state);
    koreader_sync::start_from_settings(&state.koreader_sync, &koreader_sync_deps).await;
    app.manage(state);

    Ok(())
}

/// The single Specta builder: the command list every consumer shares.
///
/// `run` uses it for the invoke handler and the debug bindings export, and
/// `bin/generate-bindings` uses it for the same export without booting the
/// GUI. One list, so a new command cannot exist at runtime but be missing
/// from the generated `bindings.ts`.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::catalog::get_edition_detail,
        commands::catalog::get_edition_covers,
        commands::identifier_edit::remove_edition_identifier,
        commands::identifier_edit::update_edition_identifier,
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
        commands::import::create_virtual_book,
        commands::import::search_biblio_editions,
        commands::import::relink_edition_file,
        commands::import::get_shelf_placement,
        commands::import::sync_shelf_placement,
        commands::calibre::import_calibre_library,
        commands::plugins::list_plugins,
        commands::plugins::add_plugin_from_path,
        commands::plugins::remove_plugin,
        commands::plugins::set_plugin_enabled,
        commands::plugins::discover_remote_plugins,
        commands::plugins::install_remote_plugin,
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
        commands::reader::save_reader_position,
        commands::reader::get_reader_position,
        commands::reader::add_reader_bookmark,
        commands::reader::remove_reader_bookmark,
        commands::reader::list_reader_bookmarks,
        commands::sync::sync_health,
        commands::sync::sync_status,
        commands::sync::sync_pairing_begin,
        commands::sync::sync_pairing_list,
        commands::sync::sync_nearby_devices,
        commands::sync::sync_device_name_get,
        commands::sync::sync_device_name_save,
        commands::sync::sync_send_book,
        commands::sync::sync_pull,
        commands::sync::sync_offers_list,
        commands::sync::sync_offers_decide,
        commands::sync::sync_offer_cover,
        commands::sync::sync_pair_claim,
        commands::sync::sync_pairing_approve,
        commands::sync::sync_pairing_reject,
        commands::sync::sync_devices_list,
        commands::sync::sync_devices_revoke,
        commands::sync::sync_diagnostics,
        firewall::sync_firewall_hint,
        firewall::sync_firewall_allow,
        commands::sync::sync_conflicts_list,
        commands::sync::sync_conflicts_resolve,
        commands::atproto::atproto_begin_login,
        commands::atproto::atproto_complete_login,
        commands::atproto::atproto_status,
        commands::atproto::atproto_logout,
        commands::atproto::atproto_refresh_session,
        commands::atproto::atproto_get_profile,
        commands::atproto::atproto_update_profile,
        commands::atproto::atproto_set_avatar,
        commands::atproto_shelves::list_shelved_books,
        commands::atproto_shelves::import_shelved_book,
        commands::reviews::save_local_review,
        commands::reviews::load_local_reviews,
        commands::reviews::remove_local_review,
        commands::reviews::put_local_review_to_pds,
        commands::reading_lists::list_reading_lists,
        commands::reading_lists::get_reading_list,
        commands::reading_lists::update_reading_list,
        commands::reading_lists::reorder_reading_list,
        commands::reading_lists::create_reading_list,
        commands::reading_lists::delete_reading_list,
        commands::reading_lists::add_reading_list_book,
        commands::reading_lists::remove_reading_list_book,
        commands::mcp::mcp_settings_get,
        commands::mcp::mcp_settings_save,
        commands::opds_server::opds_server_status,
        commands::opds_server::opds_server_settings_save,
        commands::opds_server::opds_server_start,
        commands::opds_server::opds_server_stop,
        commands::spotlight::spotlight_status,
        commands::spotlight::spotlight_set_enabled,
        commands::spotlight::spotlight_reindex,
        commands::koreader_sync::koreader_sync_status,
        commands::koreader_sync::koreader_sync_settings_save,
        commands::koreader_sync::koreader_sync_start,
        commands::koreader_sync::koreader_sync_stop,
        commands::enrich::apply_enrichment,
        commands::enrich::enrich_library,
        commands::enrich::preview_enrichment,
    ])
}

// `tauri::generate_context!` expands to code using `std::path::PathBuf`;
// the allow is scoped here because the macro body cannot be edited.
#[allow(clippy::disallowed_types)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta_builder = specta_builder();

    let builder = tauri::Builder::default();

    // Must be the first plugin: a second launch focuses the running window
    // instead of starting a new app.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            // A second launch must also reveal a window hidden in the tray.
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        crate::tray::sync_visibility(app);
    }));

    let builder = builder
        .plugin(tauri_plugin_deep_link::init())
        // `StoreBuilder::new` reads `StoreState` from the app, which only
        // exists once this plugin is registered; `app_setup` builds the OPDS
        // store, so registration must happen before setup runs.
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_decorum::init())
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
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                // Keep the app (and its sidecars) alive in the tray; quitting
                // is the tray menu's job once the window is hidden.
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                {
                    api.prevent_close();
                    let _ = window.hide();
                    crate::tray::sync_visibility(window.app_handle());
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                let _ = api;
                return;
            }
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
            .unwrap_or_else(|e| panic!("export bindings: {e}"));
    }

    #[cfg(feature = "e2e-testing")]
    let builder = builder.plugin(tauri_plugin_playwright::init());

    // Expose the dev automation bridge (local WebSocket) only in debug builds;
    // it grants arbitrary JS/IPC control and must never ship in release.
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    // One handler for every native menu, tray or application menu: Tauri
    // dispatches menu events globally, so a second listener would run twice.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let builder = builder.on_menu_event(|app, event| {
        crate::menu::handle_event(app, event.id().as_ref());
    });

    let app = builder
        .setup(move |app| {
            use tauri_plugin_decorum::WebviewWindowExt;

            if let Some(window) = app.get_webview_window("main") {
                window.create_overlay_titlebar()?;

                #[cfg(target_os = "macos")]
                {
                    window.set_traffic_lights_inset(12.0, 16.0)?;
                    window.make_transparent()?;
                    // Floating above other windows by default, so the reader
                    // stays on top when opened as a second window.
                    window.set_window_level(25)?;
                    // Without a shadow the overlay titlebar floats in space.
                    window.set_shadow(true)?;
                    // JS-side detects macOS (RouteShell.svelte); no eval needed
                }
            }

            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                crate::tray::init(app.handle())?;
                crate::menu::install(app.handle())?;
            }

            #[cfg(target_os = "macos")]
            crate::spotlight::install_activation(app.handle());

            tauri::async_runtime::block_on(app_setup(app))?;

            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;

                // Dev builds and Windows installs must register the scheme at
                // runtime; bundled Linux packages ship it in the desktop file.
                #[cfg(any(target_os = "linux", target_os = "windows"))]
                if let Err(error) = app.deep_link().register_all() {
                    tracing::warn!(%error, "could not register the ATProto callback scheme");
                }

                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    crate::commands::atproto::handle_deep_links(&handle, event.urls());
                });

                if let Some(urls) = app.deep_link().get_current()? {
                    crate::commands::atproto::handle_deep_links(app.handle(), urls);
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| panic!("error while running tauri application: {e}"));

    app.run(|app, event| {
        #[cfg(target_os = "macos")]
        match event {
            // The AppKit delegate is guaranteed to exist once the loop runs;
            // setup installs the hook when it is already too early to do so.
            tauri::RunEvent::Ready => crate::spotlight::install_activation(app),
            // Clicking the Dock icon with every window hidden must bring the
            // main window back; macOS otherwise leaves the app windowless.
            tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => crate::tray::focus_main_window(app),
            _ => {}
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}

// Test fixture for bulk wrapper tests (adds mock_state_for_bulk)
mod test_fixture;

pub use test_fixture::mock_state_for_bulk;
