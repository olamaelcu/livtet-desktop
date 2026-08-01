//! Language preference for remote search — stored as a tiny JSON
//! sidecar at `app_config_dir()/language_preference.json`.
//!
//! OpenLibrary returns results in every language regardless of the
//! user's locale. This setting lets the user filter by a preferred
//! language (ISO 639-3 code, e.g. "eng", "fra", "spa"). `null`
//! means no filter.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};

const META_FILE: &str = "language_preference.json";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct LanguagePreference {
    pub language: Option<String>,
}

fn metadata_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(META_FILE))
}

fn read_preference(app: &AppHandle) -> LanguagePreference {
    let Ok(path) = metadata_path(app) else {
        return LanguagePreference { language: None };
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return LanguagePreference { language: None };
    };
    serde_json::from_str::<LanguagePreference>(&raw).unwrap_or(LanguagePreference { language: None })
}

fn write_preference(app: &AppHandle, pref: &LanguagePreference) -> Result<(), String> {
    let path = metadata_path(app)?;
    let body = serde_json::to_string(pref).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn get_language_preference(app: AppHandle) -> Result<LanguagePreference, String> {
    Ok(read_preference(&app))
}

#[tauri::command]
#[specta::specta]
pub async fn set_language_preference(
    app: AppHandle,
    language: Option<String>,
) -> Result<LanguagePreference, String> {
    let pref = LanguagePreference {
        language: language.filter(|l| !l.is_empty()),
    };
    write_preference(&app, &pref)?;
    Ok(pref)
}
