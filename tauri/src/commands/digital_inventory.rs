//! Digital-inventory lookup commands + the `DigitalInventoryRow` specta wrapper.
//!
//! `livtet_data::entities::digital_inventory::Model` (the SeaORM-generated
//! row type) does not derive `specta::Type`. `DigitalInventoryRow` is a
//! minimal newtype around the model's columns we want to expose across
//! the IPC boundary, with the derive macros needed for tauri-specta to
//! generate a TS type.

use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// Digital-inventory fields exposed to the webview. Mirrors the columns
/// we want in the detail view's Files tab; add fields here when the UI
/// asks for them, not preemptively.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DigitalInventoryRow {
    pub id: String,
    pub edition_id: String,
    pub file_path: Option<String>,
    pub cover_path: Option<String>,
    pub blurhash: Option<String>,
    pub dominant_color: Option<String>,
    pub file_hash: Option<String>,
    /// File size in bytes. Stored as `f64` for the IPC boundary so
    /// specta-typescript can export it as a JSON `number`; values up
    /// to 2^53 (~9 PB) round-trip exactly, which covers any real file.
    pub file_size_bytes: Option<f64>,
    /// File format (e.g., "EPUB", "PDF"). Derived from editions.format_id.
    pub file_format: Option<String>,
    pub notes: Option<String>,
    pub added_at: String,
    pub updated_at: Option<String>,
}

impl From<livtet_core::data::entities::digital_inventory::Model> for DigitalInventoryRow {
    fn from(m: livtet_core::data::entities::digital_inventory::Model) -> Self {
        Self {
            id: m.id.to_string(),
            edition_id: m.edition_id.to_string(),
            file_path: m.file_path,
            cover_path: m.cover_path,
            blurhash: m.blurhash,
            dominant_color: m.dominant_color,
            file_hash: m.file_hash,
            file_size_bytes: m.file_size_bytes.map(|n| n as f64),
            file_format: m.file_format,
            notes: m.notes,
            added_at: m.added_at.to_string(),
            updated_at: m.updated_at.map(|d| d.to_string()),
        }
    }
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err, fields(edition_id))]
pub async fn find_files_by_edition(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<Option<DigitalInventoryRow>, String> {
    let db = state.db.db_conn();
    let edition_id = edition_id
        .parse::<livtet_core::DbId>()
        .map_err(|e| format!("invalid id: {e}"))?;
    let row = livtet_core::data::entities::digital_inventory::Entity::find()
        .filter(livtet_core::data::entities::digital_inventory::Column::EditionId.eq(edition_id))
        .one(&db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(DigitalInventoryRow::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::DbId;
    use livtet_core::data::TestDb;
    use livtet_core::data::entities::{digital_inventory, editions, works};
    use livtet_core::data::orm::{ActiveModelTrait, DatabaseConnection, Set};
    use time::PrimitiveDateTime;

    fn now() -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            time::Date::from_calendar_date(2026, time::Month::January, 1).unwrap(),
            time::Time::MIDNIGHT,
        )
    }

    async fn seed_work(db: &DatabaseConnection) -> works::Model {
        works::ActiveModel {
            id: Set(DbId::new()),
            title: Set("Test Work".into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_edition(db: &DatabaseConnection, work_id: DbId) -> editions::Model {
        editions::ActiveModel {
            id: Set(DbId::new()),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Test Edition".into())),
            published_date: Set(None),
            format_id: Set(None),
            language_id: Set(None),
            notes: Set(None),
            description: Set(None),
            created_at: Set(now()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_file(
        db: &DatabaseConnection,
        edition_id: DbId,
        file_path: &str,
    ) -> digital_inventory::Model {
        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(Some(file_path.into())),
            cover_path: Set(None),
            blurhash: Set(Some("LKO2?U%2Tw=w]~RBVZRi};RPxuwH".into())),
            dominant_color: Set(Some("#6b7e8a".into())),
            file_hash: Set(Some("abc12345def67890".into())),
            file_size_bytes: Set(Some(1024)),
            file_format: Set(Some("EPUB".into())),
            notes: Set(None),
            added_at: Set(now()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_files_by_edition_returns_seeded_row() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let file = seed_file(&sea, edition.id, "/tmp/book.epub").await;

        let found = find_files_by_edition_for_test(&sea, edition.id.to_string())
            .await
            .unwrap();
        assert_eq!(found.as_ref().unwrap().id, file.id.to_string());
        assert_eq!(
            found.as_ref().unwrap().file_path.as_deref(),
            Some("/tmp/book.epub")
        );
    }

    #[tokio::test]
    async fn find_files_by_edition_returns_none_when_no_files() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let result = find_files_by_edition_for_test(&sea, DbId::new().to_string())
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // Test-only helper: bypasses Tauri's `State<'_, AppState>` so we
    // can exercise the lookup logic against an in-memory DB without
    // spinning up a Tauri runtime.
    async fn find_files_by_edition_for_test(
        db: &DatabaseConnection,
        edition_id: String,
    ) -> Result<Option<DigitalInventoryRow>, String> {
        let edition_id = edition_id.parse::<DbId>().map_err(|e| e.to_string())?;
        let row = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(DigitalInventoryRow::from))
    }
}
