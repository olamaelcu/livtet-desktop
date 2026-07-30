use std::sync::OnceLock;

use camino::{Utf8Path, Utf8PathBuf};
use miette::IntoDiagnostic;
use tauri::{App, Manager};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_forest::{ForestLayer, traits::*, util::EnvFilter};
use tracing_subscriber::fmt;

#[derive(thiserror::Error, miette::Diagnostic, Debug)]
pub enum Error {
    #[error("Could not obtain the UTF-8 version of this path {0:?}")]
    PathResolution(std::path::PathBuf),
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[derive(Debug)]
struct AppDirectories {
    database_path: Utf8PathBuf,
    logs_dir: Utf8PathBuf,
}

impl AppDirectories {
    #[tracing::instrument(skip(app), ret, err)]
    async fn resolve(app: &App) -> miette::Result<Self> {
        let path_resolver = app.path();
        let local_data_dir_path =
            Utf8PathBuf::from_path_buf(path_resolver.app_local_data_dir().into_diagnostic()?)
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

        Ok(Self {
            database_path,
            logs_dir,
        })
    }
}

static LOG_FILE_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

fn init_tracing(logs_dir: &Utf8Path) -> miette::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let filter = env_filter
        .add_directive("tokio_tungstenite=off".parse().into_diagnostic()?)
        .add_directive("tokio_tungstenite::compat=off".parse().into_diagnostic()?);

    let file_appender =
        RollingFileAppender::new(Rotation::DAILY, logs_dir, "livtet.log");
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
        "Connecting to the database..."
    );

    if fs_err::tokio::try_exists(&paths.database_path).await.ok() != Some(true) {
        fs_err::tokio::write(&paths.database_path, &[]).await?;
    }

    let db = livtet_core::data::SharedState::connect(paths.database_path.as_str()).await?;
    app.manage(db);

    // FIXME: Set up the database as state.
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_decorum::init());

    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    builder
        .setup(|app| {
            use tauri::Manager;
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
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
