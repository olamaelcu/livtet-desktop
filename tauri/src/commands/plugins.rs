use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::state::AppState;

#[derive(Serialize, Type, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub loaded: bool,
    pub capabilities: Vec<String>,
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    let host = state.plugin_host.lock().await;
    let plugins = host.list_plugins();
    let result = plugins
        .into_iter()
        .map(|manifest| {
            let id = &manifest.plugin.id;
            PluginInfo {
                id: id.clone(),
                name: manifest.plugin.name.clone(),
                version: manifest.plugin.version.clone(),
                description: manifest.plugin.description.clone().unwrap_or_default(),
                enabled: !host.is_disabled(id),
                loaded: host.is_active_version_loaded(id),
                capabilities: manifest
                    .plugin
                    .capabilities
                    .iter()
                    .filter(|(_, enabled)| **enabled)
                    .map(|(cap, _)| cap.as_str().to_string())
                    .collect(),
            }
        })
        .collect();
    Ok(result)
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn load_plugin(
    state: State<'_, AppState>,
    plugin_id: String,
    version: String,
) -> Result<(), String> {
    let mut host = state.plugin_host.lock().await;
    host.load_plugin(&plugin_id, &version)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn unload_plugin(
    state: State<'_, AppState>,
    plugin_id: String,
    version: String,
) -> Result<(), String> {
    let mut host = state.plugin_host.lock().await;
    host.unload_plugin(&plugin_id, &version)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn set_plugin_disabled(
    state: State<'_, AppState>,
    plugin_id: String,
    disabled: bool,
) -> Result<(), String> {
    let mut host = state.plugin_host.lock().await;
    host.set_disabled(&plugin_id, disabled);
    Ok(())
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn call_plugin_capability(
    state: State<'_, AppState>,
    plugin_id: String,
    capability: String,
    args: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let mut host = state.plugin_host.lock().await;
    host.call(&plugin_id, &capability, args)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn get_plugin_setting(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
) -> Result<Option<String>, String> {
    let host = state.plugin_host.lock().await;
    Ok(host.get_setting(&plugin_id, &key).await)
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn write_plugin_setting(
    state: State<'_, AppState>,
    plugin_id: String,
    key: String,
    value: String,
) -> Result<(), String> {
    let host = state.plugin_host.lock().await;
    host.write_setting_direct(&plugin_id, &key, &value)
        .await
        .map_err(|e| e.to_string())
}
