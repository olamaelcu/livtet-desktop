use serde::{Deserialize, Serialize};
use specta::Type;
use specta::datatype::DataType;
use specta::Types;
use tauri::State;

use crate::state::AppState;

/// Specta-safe wrapper around `serde_json::Value`.
///
/// `serde_json::Value` is a recursive enum, which makes the
/// `specta_typescript` exporter recurse into a `RecursiveInlineType`
/// it can't serialize — that crashes `generate-bindings` with a
/// runtime stack overflow. Wrapping it here and providing a custom
/// `Type` impl that renders as `any` sidesteps the recursion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginJson(pub serde_json::Value);

impl Type for PluginJson {
    fn definition(_: &mut Types) -> DataType {
        DataType::Reference(specta_typescript::define("any"))
    }
}

/// Typed invocation of a plugin capability.
///
/// One variant per typed dispatch method on
/// `livtet_plugins::host_manager::PluginHostManager`. The frontend
/// sees a discriminated union in `bindings.ts` so the wire shape of
/// each capability is checked at compile time instead of relying on
/// the raw `(capability: string, args: any[])` bridge.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CapabilityCall {
    // ── catalog_resolver ──
    CatalogResolver { url: String },

    // ── reading_progress ──
    ProgressSources,
    FetchProgress {
        source_id: String,
        config: PluginJson,
    },

    // ── annotations ──
    AnnotationSources,
    FetchAnnotations {
        source_id: String,
        config: PluginJson,
    },

    // ── reading_list ──
    ListSources,
    FetchLists {
        source_id: String,
        config: PluginJson,
    },

    // ── series ──
    DetectSeries { edition_info: PluginJson },
    DetectSeriesBatch { editions_array: PluginJson },
    GetSeriesOrder { series_info: PluginJson },

    // ── search / lookup / enrich / cover ──
    Search {
        query: String,
        options: PluginJson,
    },
    Lookup { identifier: String },
    Enrich { work_info: PluginJson },
    GetCover {
        work_info: PluginJson,
        edition_info: Option<PluginJson>,
    },

    // ── watch ──
    Watch { since: Option<String> },

    // ── import ──
    ImportDetect { source: PluginJson },
    ImportListItems {
        source: PluginJson,
        options: PluginJson,
    },
    ImportItems {
        source: PluginJson,
        options: PluginJson,
    },
}

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
#[tracing::instrument(skip(state, call), err)]
pub async fn call_plugin_capability(
    state: State<'_, AppState>,
    plugin_id: String,
    call: CapabilityCall,
) -> Result<PluginJson, String> {
    let mut host = state.plugin_host.lock().await;
    let result = match call {
        CapabilityCall::CatalogResolver { url } => host
            .resolve_catalog_url(&plugin_id, &url)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or(serde_json::Value::Null),

        CapabilityCall::ProgressSources => host
            .call_progress_sources(&plugin_id)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::FetchProgress { source_id, config } => host
            .call_fetch_progress(&plugin_id, &source_id, config.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::AnnotationSources => host
            .call_annotation_sources(&plugin_id)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::FetchAnnotations { source_id, config } => host
            .call_fetch_annotations(&plugin_id, &source_id, config.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::ListSources => host
            .call_list_sources(&plugin_id)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::FetchLists { source_id, config } => host
            .call_fetch_lists(&plugin_id, &source_id, config.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::DetectSeries { edition_info } => host
            .call_detect_series(&plugin_id, edition_info.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::DetectSeriesBatch { editions_array } => host
            .call_detect_series_batch(&plugin_id, editions_array.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::GetSeriesOrder { series_info } => host
            .call_get_series_order(&plugin_id, series_info.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::Search { query, options } => host
            .call_search(&plugin_id, &query, options.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::Lookup { identifier } => host
            .call_lookup(&plugin_id, &identifier)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::Enrich { work_info } => host
            .call_enrich(&plugin_id, work_info.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::GetCover {
            work_info,
            edition_info,
        } => host
            .call_get_cover(
                &plugin_id,
                work_info.0,
                edition_info.map(|e| e.0),
            )
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::Watch { since } => {
            let raw = host
                .call_watch(&plugin_id, since)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(raw).map_err(|e| e.to_string())?
        }

        CapabilityCall::ImportDetect { source } => host
            .call_import_detect(&plugin_id, source.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::ImportListItems { source, options } => host
            .call_import_list_items(&plugin_id, source.0, options.0)
            .await
            .map_err(|e| e.to_string())?,

        CapabilityCall::ImportItems { source, options } => host
            .call_import_items(&plugin_id, source.0, options.0)
            .await
            .map_err(|e| e.to_string())?,
    };

    Ok(PluginJson(result))
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
