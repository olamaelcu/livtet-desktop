//! Review commands: SQLite storage and PDS sync
//!
//! Reviews are persisted locally via SQLite (`reading_progress` table is
//! reused for the review text; the PDS sync writes a `net.olamaelcu.livtet.biblio`
//! record when a session is available. SQLite writes never roll back on sync
//! failure — the local review always survives.

use crate::{atproto, types::AppState};
use tauri::State;

/// Persisted local review: text + optional 0–10 rating.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct LocalReview {
    pub id: String,
    pub edition_id: String,
    pub text: String,
    pub rating: Option<i32>,
    pub created_at: String,
}

/// Save a review locally. Inserts into `reading_progress` with `progress_unit = "review"`.
#[tauri::command]
#[specta::specta]
pub async fn save_local_review(
    edition_id: String,
    text: String,
    rating: Option<i32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let db = &state.db.db_conn();
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
    use crate::data::orm::reading_progress::{ActiveModel, Entity as ReadingProgress, Column};

    // Find existing row for this edition
    let existing = Entity::find()
        .filter(Column::EditionId.eq(&edition_id))
        .one(db)
        .await
        .map_err(|e| format!("Failed to query reading_progress: {}", e))?;

    let now = chrono::Utc::now().to_rfc3339();

    if let Some(row) = existing {
        let mut active: ActiveModel = row.into();
        active.last_location = Set(Some(text.clone()));
        active.annotations = Set(Some(serde_json::json!({
            "type": "review",
            "text": text,
            "rating": rating,
            "created_at": now,
        }).to_string()));
        active.updated_at = Set(Some(now));
        active.progress_unit = Set(Some("review".to_string()));
        active.progress = Set(0.0);
        active.exec_with_returning(db)
            .await
            .map_err(|e| format!("Failed to update review: {}", e))?;
    } else {
        let active = ActiveModel {
            id: Set(uuid::Uuid::new_v4().to_string()),
            edition_id: Set(edition_id),
            format_id: Set(String::new()),
            progress: Set(0.0),
            progress_unit: Set(Some("review".to_string())),
            last_location: Set(Some(text)),
            annotations: Set(Some(serde_json::json!({
                "type": "review",
                "text": text,
                "rating": rating,
                "created_at": now,
            }).to_string())),
            created_at: Set(Some(now)),
            updated_at: Set(Some(now)),
        };
        active.exec_with_returning(db)
            .await
            .map_err(|e| format!("Failed to insert review: {}", e))?;
    }

    Ok(())
}

/// Push a review to the user's PDS as a `net.olamaelcu.livtet.biblio` record.
#[tauri::command]
#[specta::specta]
pub async fn put_local_review_to_pds(
    edition_id: String,
    text: String,
    rating: Option<i32>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    atproto::put_record(&edition_id, &text, rating, state).await
}