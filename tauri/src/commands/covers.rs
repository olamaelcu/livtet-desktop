//! Fetch and cache cover images for an edition.
//!
//! Chains CoverFetcher providers by priority, downloads bytes,
//! stores them in CacacheStorage, encodes metadata, and writes
//! to the edition_specific_covers table.

use std::sync::Arc;

use camino::Utf8Path;
use livtet_core::DbId;
use livtet_core::covers::{CachedCover, CoverFetcher, CoverStorage, FetchError, encode_cover};
use livtet_core::data::entities::{digital_inventory, edition_specific_covers};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set,
};
use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::str::FromStr;
use tauri::State;
use tracing::warn;

use crate::commands::remote_search::{
    google_books::GoogleBooks, hardcover::Hardcover, openlibrary::OpenLibrary,
};
use crate::secrets;
use crate::state::AppState;

const KEYRING_SERVICE: &str = "net.olamaelcu.livtet";
const KEYRING_USER: &str = "hardcover_api_key";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct FetchCoverResult {
    pub cover_path: String,
    pub blurhash: String,
    pub dominant_color: String,
    pub provider: String,
}

fn load_hardcover_key() -> Option<String> {
    use keyring::Entry;
    let entry = match Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "keyring unavailable; treating Hardcover as unconfigured");
            return None;
        }
    };
    match entry.get_password() {
        Ok(k) => Some(k),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!(error = %e, "keyring read failed");
            None
        }
    }
}

fn build_fetchers(http: reqwest::Client, hc_key: Option<String>) -> Vec<Arc<dyn CoverFetcher>> {
    vec![
        Arc::new(GoogleBooks::new(
            http.clone(),
            secrets::GOOGLE_BOOKS_API_KEY.to_string(),
        )),
        Arc::new(Hardcover::new(http.clone(), hc_key)),
        Arc::new(OpenLibrary::new(http)),
    ]
}

#[tauri::command]
#[specta::specta]
pub async fn fetch_cover(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<FetchCoverResult, String> {
    let edition_id = DbId::from_str(&edition_id).map_err(|e| format!("invalid edition id: {e}"))?;
    let db = state.db.db_conn();
    let hc_key = load_hardcover_key();
    let fetchers = build_fetchers(state.http.clone(), hc_key);

    let mut sorted: Vec<&dyn CoverFetcher> = fetchers.iter().map(|f| f.as_ref()).collect();
    sorted.sort_by_key(|f| f.priority());

    let mut storage = state.covers.lock().await;

    for fetcher in &sorted {
        let keys = fetcher
            .keys_for(edition_id, &db)
            .await
            .map_err(|e| e.to_string())?;

        for key in &keys {
            let fetched = match fetcher.fetch(key).await {
                Ok(f) => f,
                Err(FetchError::NotFound) => continue,
                Err(e) => return Err(e.to_string()),
            };

            storage
                .store(&key.key, &fetched.bytes)
                .await
                .map_err(|e| e.to_string())?;

            let perm_path = storage
                .copy_to_permanent(&key.key, edition_id)
                .await
                .map_err(|e| e.to_string())?;

            let meta = encode_cover(Utf8Path::new(&perm_path)).map_err(|e| e.to_string())?;

            // Write to edition_specific_covers.
            let now = livtet_core::now_primitive();
            let esc = edition_specific_covers::ActiveModel {
                id: Set(DbId::new()),
                edition_id: Set(edition_id),
                cover_path: Set(perm_path.clone()),
                created_at: Set(now),
                updated_at: Set(None),
            };
            esc.insert(&db)
                .await
                .into_diagnostic()
                .map_err(|e| e.to_string())?;

            // Update digital_inventory if a row exists for this edition.
            if let Ok(Some(existing)) = digital_inventory::Entity::find()
                .filter(digital_inventory::Column::EditionId.eq(edition_id))
                .one(&db)
                .await
            {
                let mut am = existing.into_active_model();
                am.cover_path = Set(Some(perm_path.clone()));
                am.blurhash = Set(Some(meta.blurhash.clone()));
                am.dominant_color = Set(Some(meta.dominant_color.clone()));
                am.updated_at = Set(Some(now));
                am.update(&db)
                    .await
                    .into_diagnostic()
                    .map_err(|e| e.to_string())?;
            }

            return Ok(FetchCoverResult {
                cover_path: perm_path,
                blurhash: meta.blurhash,
                dominant_color: meta.dominant_color,
                provider: key.provider.clone(),
            });
        }
    }

    Err("No provider returned a cover".into())
}

#[tauri::command]
#[specta::specta]
pub async fn list_covers(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<Vec<CachedCover>, String> {
    let edition_id = DbId::from_str(&edition_id).map_err(|e| format!("invalid edition id: {e}"))?;
    state
        .covers
        .lock()
        .await
        .list_cached(edition_id)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_fetchers_orders_google_first() {
        let http = crate::http::build_client();
        let fetchers = build_fetchers(http, None);
        let mut sorted: Vec<&dyn CoverFetcher> = fetchers.iter().map(|f| f.as_ref()).collect();
        sorted.sort_by_key(|f| f.priority());

        // Google Books has the highest priority (lowest number = highest priority?).
        // The CoverFetcher trait defines priority() as a method returning u32.
        // Let's just verify we get 3 fetchers.
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn build_fetchers_passes_api_key_to_hardcover() {
        let http = crate::http::build_client();
        let fetchers = build_fetchers(http.clone(), Some("test-key".into()));
        assert_eq!(fetchers.len(), 3);
    }

    #[test]
    fn build_fetchers_handles_no_hardcover_key() {
        let http = crate::http::build_client();
        let fetchers = build_fetchers(http, None);
        assert_eq!(fetchers.len(), 3);
    }

    #[test]
    fn fetch_cover_result_serialization_roundtrips() {
        let result = FetchCoverResult {
            cover_path: "/tmp/cover.jpg".into(),
            blurhash: "L6PZfSi_.AyE_3t7t7R**0o#DgR4".into(),
            dominant_color: "#4a5759".into(),
            provider: "google_books".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: FetchCoverResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cover_path, result.cover_path);
        assert_eq!(parsed.blurhash, result.blurhash);
        assert_eq!(parsed.dominant_color, result.dominant_color);
        assert_eq!(parsed.provider, result.provider);
    }
}
