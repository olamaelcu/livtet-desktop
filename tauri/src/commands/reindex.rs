//! Reindex command — rebuilds the Tantivy search index from the
//! database without restarting the app.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct ReindexComplete;

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn reindex(state: State<'_, AppState>) -> Result<(), String> {
    tracing::info!("reindex requested by user");
    let db_conn = state.db.db_conn();
    let path = state.search_index_path.clone();

    let old_version = livtet_core::search::SearchIndex::migrate_to(path.as_path(), &db_conn)
        .await
        .map_err(|e| e.to_string())?;

    let new_index =
        livtet_core::search::SearchIndex::open(path.as_path()).map_err(|e| e.to_string())?;

    {
        let mut search = state.search.write().await;
        *search = new_index;
    }

    tracing::info!(
        old_version = ?old_version,
        "reindex complete"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindex_complete_event_serializes() {
        let json = serde_json::to_string(&ReindexComplete).unwrap();
        assert_eq!(json, "null");
    }

    #[test]
    fn reindex_complete_event_deserializes() {
        let event: ReindexComplete = serde_json::from_str("null").unwrap();
        let _ = event;
    }

    #[test]
    fn reindex_command_signature_is_valid() {
        fn _check(
            state: tauri::State<'_, crate::state::AppState>,
        ) -> impl std::future::Future<Output = Result<(), String>> {
            reindex(state)
        }
    }
}
