use camino::Utf8PathBuf;
use miette::IntoDiagnostic;
use tauri::{App, Manager};

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

        let database_path = local_data_dir_path.join("livtet.sqlite");

        Ok(Self { database_path })
    }
}

#[tracing::instrument(skip(app), ret, err)]
async fn app_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error + 'static>> {
    tracing_forest::init();
    let paths = AppDirectories::resolve(app).await?;
    tracing::trace!(
        db_path = paths.database_path.to_string(),
        "Connecting to the database..."
    );

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
