//! Font management commands.
//!
//! Downloads, deletes, and lists font families from the
//! `@fontsource-variable` npm package via the jsDelivr CDN. Each
//! family is stored under `fonts_dir/{family_id}@{version}/` with a
//! `manifest.json` sidecar describing the downloaded files.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// A manifest describing a font family that has been (or will be)
/// downloaded to the local font cache.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FontManifest {
    pub family_id: String,
    pub family: String,
    pub version: String,
    pub variable: bool,
    pub files: Vec<FontFileEntry>,
}

/// A single font file within a [`FontManifest`], keyed by subset and
/// style.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FontFileEntry {
    pub subset: String,
    pub style: String,
    pub path: String,
}

/// A lightweight handle for a downloaded font family, returned by
/// [`list_downloaded_fonts`].
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FontHandle {
    pub family_id: String,
    pub family: String,
    pub version: String,
}

fn is_safe_family_id(id: &str) -> bool {
    !id.is_empty() && !id.contains('/') && !id.contains('.')
}

fn family_id_to_family(id: &str) -> String {
    id.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(dead_code)]
fn make_font_dir_path(base: &Utf8Path, family_id: &str, version: &str) -> Utf8PathBuf {
    base.join("fonts").join(format!("{family_id}@{version}"))
}

fn build_cdn_url(family_id: &str, version: &str, subset: &str, style: &str) -> String {
    format!(
        "https://cdn.jsdelivr.net/npm/@fontsource-variable/{family_id}@{version}/files/{family_id}-{subset}-wght-{style}.woff2"
    )
}

#[tauri::command]
#[specta::specta]
pub async fn download_font(
    state: State<'_, AppState>,
    family_id: String,
    subsets: Vec<String>,
    styles: Vec<String>,
) -> Result<FontManifest, String> {
    if !is_safe_family_id(&family_id) {
        return Err(format!("invalid family_id: {family_id}"));
    }

    let registry_url = format!("https://registry.npmjs.org/@fontsource-variable/{family_id}");
    tracing::trace!(url = %registry_url, "Resolving npm package version");
    let registry_resp = state
        .http
        .get(registry_url.as_str())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !registry_resp.status().is_success() {
        return Err(format!(
            "npm registry request failed: {}",
            registry_resp.status()
        ));
    }
    let registry_json: serde_json::Value = registry_resp
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let version = registry_json
        .pointer("/dist-tags/latest")
        .and_then(|v| v.as_str())
        .ok_or("could not resolve latest version from npm registry")?
        .to_string();

    let font_dir = state.fonts_dir.join(format!("{family_id}@{version}"));
    fs_err::tokio::create_dir_all(&font_dir)
        .await
        .map_err(|e| e.to_string())?;

    let family = family_id_to_family(&family_id);
    let mut files = Vec::new();

    for subset in &subsets {
        for style in &styles {
            let url = build_cdn_url(&family_id, &version, subset, style);
            let dest = font_dir.join(format!("{family_id}-{subset}-wght-{style}.woff2"));

            tracing::trace!(url = %url, "Downloading font file");
            let resp = state
                .http
                .get(url.as_str())
                .send()
                .await
                .map_err(|e| e.to_string())?;

            if !resp.status().is_success() {
                return Err(format!("failed to download {url}: {}", resp.status()));
            }

            let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
            fs_err::tokio::write(&dest, &bytes)
                .await
                .map_err(|e| e.to_string())?;

            files.push(FontFileEntry {
                subset: subset.clone(),
                style: style.clone(),
                path: dest.to_string(),
            });
        }
    }

    let manifest = FontManifest {
        family_id: family_id.clone(),
        family: family.clone(),
        version: version.clone(),
        variable: true,
        files: files.clone(),
    };

    let manifest_path = font_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs_err::tokio::write(&manifest_path, &manifest_json)
        .await
        .map_err(|e| e.to_string())?;

    Ok(manifest)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_font(
    state: State<'_, AppState>,
    family_id: String,
) -> Result<(), String> {
    if !is_safe_family_id(&family_id) {
        return Err(format!("invalid family_id: {family_id}"));
    }

    let prefix = format!("{family_id}@");
    let mut found = false;
    let mut to_remove = Vec::new();

    let mut entries = match fs_err::tokio::read_dir(&state.fonts_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("font family '{family_id}' not found"));
        }
        Err(e) => return Err(e.to_string()),
    };

    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) {
            found = true;
            to_remove.push(entry.path());
        }
    }

    if !found {
        return Err(format!("font family '{family_id}' not found"));
    }

    for path in to_remove {
        fs_err::tokio::remove_dir_all(&path)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_downloaded_fonts(state: State<'_, AppState>) -> Result<Vec<FontHandle>, String> {
    let mut handles = Vec::new();

    let mut entries = match fs_err::tokio::read_dir(&state.fonts_dir).await {
        Ok(entries) => entries,
        Err(_) => return Ok(handles),
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(at_pos) = name.rfind('@') {
            let family_id = &name[..at_pos];
            let version = &name[at_pos + 1..];
            if is_safe_family_id(family_id) {
                handles.push(FontHandle {
                    family_id: family_id.to_string(),
                    family: family_id_to_family(family_id),
                    version: version.to_string(),
                });
            }
        }
    }

    Ok(handles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_manifest_round_trips() {
        let font = FontManifest {
            family_id: "geist-sans".into(),
            family: "Geist Sans".into(),
            version: "1.0.0".into(),
            variable: true,
            files: vec![FontFileEntry {
                subset: "latin".into(),
                style: "regular".into(),
                path: "/fonts/geist-sans-regular.ttf".into(),
            }],
        };
        let json = serde_json::to_string(&font).unwrap();
        let parsed: FontManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.family_id, font.family_id);
        assert_eq!(parsed.family, font.family);
        assert_eq!(parsed.version, font.version);
        assert_eq!(parsed.variable, font.variable);
        assert_eq!(parsed.files.len(), font.files.len());
        assert_eq!(parsed.files[0].subset, font.files[0].subset);
        assert_eq!(parsed.files[0].style, font.files[0].style);
        assert_eq!(parsed.files[0].path, font.files[0].path);
    }

    #[test]
    fn font_file_entry_round_trips() {
        let entry = FontFileEntry {
            subset: "latin".into(),
            style: "italic".into(),
            path: "/fonts/geist-sans-italic.ttf".into(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: FontFileEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.subset, entry.subset);
        assert_eq!(parsed.style, entry.style);
        assert_eq!(parsed.path, entry.path);
    }

    #[test]
    fn font_handle_round_trips() {
        let handle = FontHandle {
            family_id: "geist-mono".into(),
            family: "Geist Mono".into(),
            version: "1.0.0".into(),
        };
        let json = serde_json::to_string(&handle).unwrap();
        let parsed: FontHandle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.family_id, handle.family_id);
        assert_eq!(parsed.family, handle.family);
        assert_eq!(parsed.version, handle.version);
    }

    #[test]
    fn is_safe_family_id_rejects_traversal() {
        assert!(!is_safe_family_id(".."));
        assert!(!is_safe_family_id("/"));
        assert!(!is_safe_family_id("."));
        assert!(!is_safe_family_id(""));
        assert!(!is_safe_family_id("geist/mono"));
        assert!(!is_safe_family_id("geist.mono"));
    }

    #[test]
    fn is_safe_family_id_accepts_valid() {
        assert!(is_safe_family_id("geist"));
        assert!(is_safe_family_id("geist-mono"));
    }

    #[test]
    fn family_id_to_family_title_cases() {
        assert_eq!(family_id_to_family("geist"), "Geist");
        assert_eq!(family_id_to_family("geist-mono"), "Geist Mono");
    }

    #[test]
    fn make_font_dir_path_works() {
        let base = Utf8Path::new("/app/data");
        let path = make_font_dir_path(base, "geist-mono", "1.0.0");
        assert_eq!(
            path,
            Utf8PathBuf::from("/app/data/fonts/geist-mono@1.0.0")
        );
    }

    #[test]
    fn build_cdn_url_works() {
        let url = build_cdn_url("geist-mono", "1.0.0", "latin", "400");
        assert_eq!(
            url,
            "https://cdn.jsdelivr.net/npm/@fontsource-variable/geist-mono@1.0.0/files/geist-mono-latin-wght-400.woff2"
        );
    }
}
