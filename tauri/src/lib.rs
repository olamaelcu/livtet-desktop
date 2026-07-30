use std::sync::{Arc, OnceLock};

use camino::Utf8Path;
use miette::IntoDiagnostic;
use tauri::{App, Manager};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_forest::{ForestLayer, traits::*, util::EnvFilter};
use tracing_subscriber::fmt;

mod commands;
pub mod state;

#[doc(hidden)]
pub mod _bindings_export {
    //! Public re-exports for the `generate-bindings` bin.
    pub use crate::commands::edition;
    pub use crate::commands::greet;
    pub use crate::commands::search;
    pub use crate::commands::window;
    pub use crate::state;
}

use commands::greet::greet;
use state::{AppDirectories, AppState};

#[derive(thiserror::Error, miette::Diagnostic, Debug)]
pub enum Error {
    #[error("Could not obtain the UTF-8 version of this path {0:?}")]
    PathResolution(std::path::PathBuf),
}

static LOG_FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

fn init_tracing(logs_dir: &Utf8Path) -> miette::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let filter = env_filter
        .add_directive("tokio_tungstenite=off".parse().into_diagnostic()?)
        .add_directive("tokio_tungstenite::compat=off".parse().into_diagnostic()?);

    let file_appender = RollingFileAppender::new(Rotation::DAILY, logs_dir, "livtet.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_FILE_GUARD.set(guard);

    tracing_subscriber::registry()
        .with(filter)
        .with(ForestLayer::default())
        .with(
            fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false)
                .with_target(true)
                .with_file(true)
                .with_line_number(true),
        )
        .init();

    Ok(())
}

#[tracing::instrument(skip(app), ret, err)]
async fn app_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    let paths = AppDirectories::resolve(app).await?;
    fs_err::tokio::create_dir_all(&paths.logs_dir).await?;
    init_tracing(&paths.logs_dir)?;
    tracing::trace!(
        db_path = paths.database_path.to_string(),
        logs_dir = paths.logs_dir.to_string(),
        search_index_path = paths.search_index_path.to_string(),
        "Connecting to the database and opening search index..."
    );

    if fs_err::tokio::try_exists(&paths.database_path).await.ok() != Some(true) {
        fs_err::tokio::write(&paths.database_path, &[]).await?;
    }

    let db = livtet_core::data::SharedState::connect(paths.database_path.as_str()).await?;
    let search = Arc::new(livtet_core::search::SearchIndex::open(
        paths.search_index_path.as_path(),
    )?);

    app.manage(AppState { db, search });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(
        tauri_specta::collect_commands![
            greet,
            commands::window::sync_window_title,
            commands::search::search,
            commands::edition::find_edition_by_id,
            commands::edition::find_edition_by_identifier,
        ],
    );

    // Bindings export runs on debug builds only. The path is
    // relative to the working directory of `tauri dev`, which is
    // the desktop repo root — so "web/lib/bindings.ts" resolves
    // correctly. Maintainers running `cargo run` from `tauri/`
    // directly will see the export fail (the path doesn't exist
    // relative to that CWD); that's intentional, not a bug.
    #[cfg(debug_assertions)]
    specta_builder
        .export(
            specta_typescript::Typescript::default(),
            "web/lib/bindings.ts",
        )
        .expect("failed to export TS bindings");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decorum::init());

    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

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
