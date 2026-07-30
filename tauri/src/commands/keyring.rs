//! Hardcover API key management via the OS keychain.
//!
//! Entries live under service `net.olamaelcu.livtet` (matches
//! `tauri/tauri.conf.json` identifier) with user `hardcover_api_key`.
//!
//! The keychain stores the secret. A small JSON sidecar at
//! `app_config_dir()/hardcover_key.json` stores the last-set
//! timestamp in RFC-3339.

use std::path::PathBuf;
use std::time::Duration;

use keyring::Entry;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use time::OffsetDateTime;

use crate::state::AppState;

const KEYRING_SERVICE: &str = "net.olamaelcu.livtet";
const KEYRING_USER: &str = "hardcover_api_key";
const META_FILE: &str = "hardcover_key.json";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct HardcoverKeyStatus {
    pub configured: bool,
    pub last_set_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct HardcoverVerifyResult {
    pub valid: bool,
    pub username: Option<String>,
    pub error: Option<String>,
}

fn entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn metadata_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(META_FILE))
}

fn read_metadata(app: &AppHandle) -> Option<String> {
    let Ok(path) = metadata_path(app) else {
        return None;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return None;
    };
    serde_json::from_str::<Meta>(&raw)
        .ok()
        .map(|m| m.last_set_at)
}

fn write_metadata(app: &AppHandle) -> Result<(), String> {
    let path = metadata_path(app)?;
    let meta = Meta {
        last_set_at: now_iso(),
    };
    let body = serde_json::to_string(&meta).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

#[derive(Serialize, Deserialize)]
struct Meta {
    last_set_at: String,
}

fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub async fn get_hardcover_key(app: AppHandle) -> Result<HardcoverKeyStatus, String> {
    let configured = entry()?.get_password().is_ok();
    Ok(HardcoverKeyStatus {
        configured,
        last_set_at: read_metadata(&app),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn set_hardcover_key(
    app: AppHandle,
    state: State<'_, AppState>,
    api_key: String,
) -> Result<HardcoverKeyStatus, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("API key is empty".into());
    }
    let verify = do_verify(state, key).await?;
    if !verify.valid {
        return Err(verify
            .error
            .unwrap_or_else(|| "key rejected by Hardcover".into()));
    }
    entry()?.set_password(key).map_err(|e| e.to_string())?;
    write_metadata(&app)?;
    get_hardcover_key(app).await
}

#[tauri::command]
#[specta::specta]
pub async fn clear_hardcover_key(app: AppHandle) -> Result<HardcoverKeyStatus, String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.to_string()),
    }
    if let Ok(path) = metadata_path(&app) {
        let _ = std::fs::remove_file(path);
    }
    Ok(HardcoverKeyStatus {
        configured: false,
        last_set_at: None,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn verify_hardcover_key(
    state: State<'_, AppState>,
    api_key: String,
) -> Result<HardcoverVerifyResult, String> {
    do_verify(state, api_key.trim()).await
}

async fn do_verify(
    state: State<'_, AppState>,
    api_key: &str,
) -> Result<HardcoverVerifyResult, String> {
    let body = serde_json::json!({ "query": "query Test { me { username } }" });
    tracing::trace!(api_key = %api_key, query = %body, "Sending test request");
    let res = state
        .http
        .post("https://api.hardcover.app/v1/graphql")
        .header("Authorization", format!("Bearer {api_key}"))
        .header(
            "User-Agent",
            "livtet-desktop/0.1.0 (+https://livtet.olamaelcu.net/apps)",
        )
        .timeout(Duration::from_secs(5))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        let body: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
        tracing::warn!(body = %body, status =%status,"Failed to handle test");
        return Ok(HardcoverVerifyResult {
            valid: false,
            username: None,
            error: Some("Invalid or expired API key".to_string()),
        });
    }
    if !status.is_success() {
        return Ok(HardcoverVerifyResult {
            valid: false,
            username: None,
            error: Some(format!("HTTP {status}")),
        });
    }
    let parsed: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    let username = extract_hardcover_username(&parsed);
    Ok(HardcoverVerifyResult {
        valid: username.is_some(),
        username,
        error: None,
    })
}

fn extract_hardcover_username(parsed: &serde_json::Value) -> Option<String> {
    parsed
        .pointer("/data/me/0/username")
        .and_then(|v| v.as_str())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_iso_is_rfc3339() {
        let s = now_iso();
        assert!(s.starts_with("20"), "expected year prefix, got {s}");
        assert!(s.contains("T"), "expected T separator, got {s}");
    }

    #[test]
    fn now_iso_never_panics() {
        let _: String = now_iso();
    }

    #[test]
    fn round_trip_metadata_via_now_iso() {
        let meta = Meta {
            last_set_at: now_iso(),
        };
        let body = serde_json::to_string(&meta).unwrap();
        let parsed: Meta = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed.last_set_at, meta.last_set_at);
    }

    #[test]
    fn extract_hardcover_username_parses_me_as_array() {
        let body = serde_json::json!({ "data": { "me": [{ "id": 1, "username": "alice" }] } });
        assert_eq!(extract_hardcover_username(&body), Some("alice".to_string()));
    }

    #[test]
    fn extract_hardcover_username_missing_me_is_none() {
        let empty_array = serde_json::json!({ "data": { "me": [] } });
        assert_eq!(extract_hardcover_username(&empty_array), None);

        let no_data = serde_json::json!({});
        assert_eq!(extract_hardcover_username(&no_data), None);

        let null_data = serde_json::json!({ "data": null });
        assert_eq!(extract_hardcover_username(&null_data), None);
    }
}
