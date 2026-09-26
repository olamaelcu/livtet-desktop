//! Listening progress for audiobook playback.
//!
//! Positions persist in `reading_progress` with `progress_unit = "timestamp"`:
//! `progress` is the seek position in seconds and `last_location` carries the
//! same position as text. `(edition_id, format_id)` is unique, so saves are
//! upserts.

use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_core::data::entities::{editions, reading_progress};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, Set,
};
use livtet_types::{DbId, KnownFormats, now_primitive};

use crate::types::AppState;

/// Listening progress errors.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PlaybackError {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl PlaybackError {
    pub(crate) fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl From<livtet_core::data::orm::DbErr> for PlaybackError {
    fn from(error: livtet_core::data::orm::DbErr) -> Self {
        Self::new("database", error)
    }
}

/// The persisted listening position for one audiobook edition.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ListeningProgress {
    pub edition_id: String,
    /// Seek position in seconds.
    pub position_seconds: f64,
    /// Total duration in seconds, when the edition carries format metadata.
    pub duration_seconds: Option<f64>,
}

/// Read the persisted listening position. `Ok(None)` when never saved.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn fetch_listening_progress(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<ListeningProgress>, PlaybackError> {
    let audiobook: DbId = KnownFormats::Audiobook.into();
    let Some(row) = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(audiobook))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let duration = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await?
        .as_ref()
        .and_then(duration_of);
    Ok(Some(ListeningProgress {
        edition_id: edition_id.to_string(),
        position_seconds: row.progress,
        duration_seconds: duration,
    }))
}

/// Save the listening position (upsert). Positions past the end clamp to the
/// duration; negative positions clamp to zero.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn store_listening_progress(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    position_seconds: f64,
) -> Result<ListeningProgress, PlaybackError> {
    if !position_seconds.is_finite() {
        return Err(PlaybackError::new("invalid", "position must be finite"));
    }
    let audiobook: DbId = KnownFormats::Audiobook.into();
    let edition = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await?
        .ok_or_else(|| PlaybackError::new("not-found", "edition not found"))?;
    if edition.format_id != Some(audiobook) {
        return Err(PlaybackError::new(
            "unsupported",
            "listening progress is for audiobook editions only",
        ));
    }
    let duration = duration_of(&edition);
    let position = match duration {
        Some(total) => position_seconds.clamp(0.0, total),
        None => position_seconds.max(0.0),
    };

    let existing = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(audiobook))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut active = row.into_active_model();
            active.progress = Set(position);
            active.last_location = Set(Some(position.to_string()));
            active.update(db).await?;
        }
        None => {
            reading_progress::ActiveModel {
                id: Set(DbId::new()),
                edition_id: Set(edition_id),
                format_id: Set(audiobook),
                progress: Set(position),
                progress_unit: Set(Some("timestamp".to_string())),
                last_location: Set(Some(position.to_string())),
                total_reading_time_secs: Set(0),
                created_at: Set(now_primitive()),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(ListeningProgress {
        edition_id: edition_id.to_string(),
        position_seconds: position,
        duration_seconds: duration,
    })
}

/// Total duration in seconds from the edition's format metadata, if present.
fn duration_of(edition: &editions::Model) -> Option<f64> {
    edition
        .format_metadata
        .as_ref()?
        .get("duration_seconds")?
        .as_i64()
        .map(|duration| duration as f64)
}

/// Read the persisted listening position. `Ok(None)` when never saved.
#[tauri::command]
#[specta::specta]
pub async fn get_listening_progress(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ListeningProgress>, PlaybackError> {
    let id = edition_id.parse::<DbId>().map_err(|_| {
        PlaybackError::new("invalid-id", format!("invalid edition id: {edition_id}"))
    })?;
    fetch_listening_progress(&state.db.db_conn(), id).await
}

/// Save the listening position (upsert). Positions past the end clamp to the
/// duration; negative positions clamp to zero.
#[tauri::command]
#[specta::specta]
pub async fn save_listening_progress(
    edition_id: String,
    position_seconds: f64,
    state: State<'_, AppState>,
) -> Result<ListeningProgress, PlaybackError> {
    let id = edition_id.parse::<DbId>().map_err(|_| {
        PlaybackError::new("invalid-id", format!("invalid edition id: {edition_id}"))
    })?;
    store_listening_progress(&state.db.db_conn(), id, position_seconds).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::{Kind, TestDb};

    async fn audiobook_edition(db: &livtet_core::data::orm::DatabaseConnection) -> DbId {
        use livtet_core::data::entities::{editions, works};
        use livtet_core::data::orm::{ActiveModelTrait, Set};
        use livtet_types::now_primitive;

        let now = now_primitive();
        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("Work".to_string()),
            description: Set(None),
            sort_title: Set(None),
            series_type: Set(None),
            language_id: Set(None),
            preferred_edition_id: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("work");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Book".to_string())),
            published_date: Set(None),
            format_id: Set(Some(KnownFormats::Audiobook.into())),
            language_id: Set(None),
            notes: Set(None),
            description: Set(None),
            format_metadata: Set(Some(serde_json::json!({
                "duration_seconds": 7200,
                "chapters": [],
            }))),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");
        edition_id
    }

    #[tokio::test]
    async fn unlistened_editions_have_no_progress() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = audiobook_edition(&db).await;

        let progress = fetch_listening_progress(&db, edition_id)
            .await
            .expect("query ok");
        assert!(progress.is_none());
    }

    #[tokio::test]
    async fn listening_positions_round_trip() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = audiobook_edition(&db).await;

        let saved = store_listening_progress(&db, edition_id, 95.5)
            .await
            .expect("save ok");
        assert_eq!(saved.edition_id, edition_id.to_string());
        assert_eq!(saved.position_seconds, 95.5);
        assert_eq!(saved.duration_seconds, Some(7200.0));

        let fetched = fetch_listening_progress(&db, edition_id)
            .await
            .expect("query ok")
            .expect("progress present");
        assert_eq!(fetched, saved);

        // A second save overwrites instead of duplicating.
        let updated = store_listening_progress(&db, edition_id, 200.0)
            .await
            .expect("resave ok");
        assert_eq!(updated.position_seconds, 200.0);
        let rows = reading_progress::Entity::find()
            .filter(reading_progress::Column::EditionId.eq(edition_id))
            .all(&db)
            .await
            .expect("query ok");
        assert_eq!(rows.len(), 1);
    }

    #[tokio::test]
    async fn listening_positions_clamp_to_the_duration() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = audiobook_edition(&db).await;

        let saved = store_listening_progress(&db, edition_id, 99_999.0)
            .await
            .expect("save ok");
        assert_eq!(saved.position_seconds, 7200.0);

        let saved = store_listening_progress(&db, edition_id, -12.0)
            .await
            .expect("save ok");
        assert_eq!(saved.position_seconds, 0.0);
    }

    #[tokio::test]
    async fn unknown_editions_fail_closed() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        store_listening_progress(&db, DbId::new(), 10.0)
            .await
            .expect_err("saving for a missing edition is an error");
    }
}
