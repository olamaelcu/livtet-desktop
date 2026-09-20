use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::PluginError;
use crate::types::AppState;

/// A plugin as reported by the out-of-process host.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PluginSummary {
    pub name: String,
    pub version: Option<String>,
    pub granted: Vec<String>,
    pub signer: String,
}

/// Lists the plugins the host loads from the application's plugin directory.
///
/// The host binary owns the registry, so it is launched, queried and dropped
/// per call: a plugin that crashes the interpreter takes the host with it, and
/// the next call gets a fresh process. `RemoteRegistry` is not `Send`, so the
/// whole exchange runs on a blocking worker and only `PluginSummary` crosses
/// back into async land.
#[tauri::command]
#[specta::specta]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginSummary>, PluginError> {
    let host = state.plugin_host_path.clone().into_std_path_buf();
    let config = state.plugin_host_config.clone().into_std_path_buf();
    let plugins_dir = state.plugins_dir.clone().into_std_path_buf();

    let plugins = tokio::task::spawn_blocking(move || {
        let options = stanchion::remote::RemoteOptions::new(host)
            .config(config)
            .plugins(&plugins_dir);
        let mut registry =
            stanchion::remote::RemoteRegistry::launch(options).map_err(PluginError::host)?;

        registry.list().map_err(PluginError::host)
    })
    .await
    .map_err(PluginError::host)??;

    Ok(plugins
        .into_iter()
        .map(|plugin| PluginSummary {
            name: plugin.name,
            version: plugin.version,
            granted: plugin.granted,
            signer: plugin.signer,
        })
        .collect())
}
