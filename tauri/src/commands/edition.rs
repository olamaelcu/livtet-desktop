//! Edition-lookup commands + the `EditionRow` specta wrapper.
//!
//! `livtet_data::entities::editions::Model` (the SeaORM-generated
//! row type) does not derive `specta::Type`. `EditionRow` is a
//! minimal newtype around the model's columns we want to expose
//! across the IPC boundary, with the derive macros needed for
//! tauri-specta to generate a TS type.

use livtet_core::data::orm::{
    ColumnTrait, EntityTrait, JoinType, QueryFilter, QueryOrder, QuerySelect, RelationTrait,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// Edition fields exposed to the webview. Mirrors the columns we
/// want in the detail view; add fields here when the UI asks for
/// them, not preemptively.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EditionRow {
    pub id: String,
    pub work_id: String,
    pub group_id: Option<String>,
    pub title: Option<String>,
    pub published_date: Option<String>,
    pub format_id: Option<String>,
    pub language_id: Option<String>,
    pub notes: Option<String>,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

impl From<livtet_core::data::entities::editions::Model> for EditionRow {
    fn from(m: livtet_core::data::entities::editions::Model) -> Self {
        Self {
            id: m.id.to_string(),
            work_id: m.work_id.to_string(),
            group_id: m.group_id.map(|x| x.to_string()),
            title: m.title,
            published_date: m.published_date.map(|d| d.to_string()),
            format_id: m.format_id.map(|x| x.to_string()),
            language_id: m.language_id.map(|x| x.to_string()),
            notes: m.notes,
            description: m.description,
            created_at: m.created_at.to_string(),
            updated_at: m.updated_at.map(|d| d.to_string()),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn find_edition_by_id(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<EditionRow>, String> {
    let db = state.db.db_conn();
    let id = id
        .parse::<livtet_core::DbId>()
        .map_err(|e| format!("invalid id: {e}"))?;
    let row = livtet_core::data::entities::editions::Entity::find_by_id(id)
        .one(&db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(EditionRow::from))
}

#[tauri::command]
#[specta::specta]
pub async fn find_edition_by_identifier(
    state: State<'_, AppState>,
    urn: String,
) -> Result<Option<EditionRow>, String> {
    use livtet_core::data::entities::{edition_identifiers, editions, identifiers};
    let db = state.db.db_conn();
    // Two-hop lookup: identifiers.value is UNIQUE, so the first
    // query gives at most one identifiers row. The junction
    // edition_identifiers is N-to-N (composite PK on
    // (edition_id, identifier_id), NOT unique on identifier_id),
    // so the second query may return multiple editions. We
    // sort by edition id for deterministic ordering and take
    // the first.
    let identifier = identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(&urn))
        .one(&db)
        .await
        .map_err(|e| e.to_string())?;
    let Some(identifier) = identifier else {
        return Ok(None);
    };

    let row = editions::Entity::find()
        .join(
            JoinType::InnerJoin,
            edition_identifiers::Relation::Edition.def().rev(),
        )
        .filter(edition_identifiers::Column::IdentifierId.eq(identifier.id))
        .order_by_asc(editions::Column::Id)
        .one(&db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(EditionRow::from))
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::DbId;
    use livtet_core::data::TestDb;
    use livtet_core::data::entities::{edition_identifiers, editions, identifiers, works};
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
            description: Set(Some("A description.".into())),
            created_at: Set(now()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_edition_by_id_returns_seeded_row() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;

        let found = find_edition_by_id_for_test(&sea, edition.id.to_string())
            .await
            .unwrap();
        assert_eq!(found.unwrap().id, edition.id.to_string());
    }

    #[tokio::test]
    async fn find_edition_by_id_returns_none_for_unknown_id() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let result = find_edition_by_id_for_test(&sea, DbId::new().to_string())
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn find_edition_by_identifier_returns_linked_edition() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let identifier = identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("isbn".into()),
            value: Set("urn:isbn:9780061120084".into()),
        }
        .insert(&sea)
        .await
        .unwrap();
        edition_identifiers::ActiveModel {
            edition_id: Set(edition.id),
            identifier_id: Set(identifier.id),
        }
        .insert(&sea)
        .await
        .unwrap();

        let found = find_edition_by_identifier_for_test(&sea, "urn:isbn:9780061120084".into())
            .await
            .unwrap();
        assert_eq!(found.unwrap().id, edition.id.to_string());
    }

    #[tokio::test]
    async fn find_edition_by_identifier_returns_none_for_unknown_urn() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let result = find_edition_by_identifier_for_test(&sea, "urn:isbn:0000000000000".into())
            .await
            .unwrap();
        assert!(result.is_none());
    }

    /// Schema-invariant guard: `edition_identifiers` has a composite
    /// primary key on `(edition_id, identifier_id)`. So the same
    /// identifier can be linked to multiple editions, but the same
    /// `(edition_id, identifier_id)` pair cannot appear twice. This
    /// test pins the composite PK constraint so a future migration
    /// that drops it would fail loudly.
    ///
    /// Note: the `find_edition_by_identifier` command relies on
    /// `identifiers.value` being UNIQUE (so the first lookup gives
    /// at most one identifiers row) and on the junction being N-to-N
    /// (so the second lookup may return multiple editions, which we
    /// sort by id for determinism). The schema invariants the command
    /// depends on are NOT pinned by this test — see
    /// `identifiers_value_is_unique` for those.
    #[tokio::test]
    async fn duplicate_junction_pair_is_rejected() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let identifier = identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("isbn".into()),
            value: Set("urn:isbn:9780061120084".into()),
        }
        .insert(&sea)
        .await
        .unwrap();

        edition_identifiers::ActiveModel {
            edition_id: Set(edition.id),
            identifier_id: Set(identifier.id),
        }
        .insert(&sea)
        .await
        .unwrap();

        // Same (edition_id, identifier_id) pair — must fail.
        let second = edition_identifiers::ActiveModel {
            edition_id: Set(edition.id),
            identifier_id: Set(identifier.id),
        }
        .insert(&sea)
        .await;
        assert!(
            second.is_err(),
            "junction must reject duplicate (edition_id, identifier_id) pair"
        );
    }

    /// Schema-invariant guard: `identifiers.value` is UNIQUE.
    /// `find_edition_by_identifier` uses this to assume the first
    /// lookup yields at most one row. If a future migration drops
    /// this UNIQUE constraint, the lookup would silently return
    /// multiple rows and the deterministic ordering would still pick
    /// the first, but the cache invalidation story would change.
    /// This test pins the UNIQUE constraint.
    #[tokio::test]
    async fn identifiers_value_is_unique() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("isbn".into()),
            value: Set("urn:isbn:9780061120084".into()),
        }
        .insert(&sea)
        .await
        .unwrap();

        let dup = identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set("isbn".into()),
            value: Set("urn:isbn:9780061120084".into()),
        }
        .insert(&sea)
        .await;
        assert!(dup.is_err(), "identifiers.value must reject duplicate URN");
    }

    // Test-only helpers: bypass Tauri's `State<'_, AppState>` so we
    // can exercise the lookup logic against an in-memory DB without
    // spinning up a Tauri runtime.
    async fn find_edition_by_id_for_test(
        db: &DatabaseConnection,
        id: String,
    ) -> Result<Option<EditionRow>, String> {
        let id = id.parse::<DbId>().map_err(|e| e.to_string())?;
        let row = editions::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(EditionRow::from))
    }

    async fn find_edition_by_identifier_for_test(
        db: &DatabaseConnection,
        urn: String,
    ) -> Result<Option<EditionRow>, String> {
        let identifier = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq(&urn))
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        let Some(identifier) = identifier else {
            return Ok(None);
        };
        let row = editions::Entity::find()
            .join(
                JoinType::InnerJoin,
                edition_identifiers::Relation::Edition.def().rev(),
            )
            .filter(edition_identifiers::Column::IdentifierId.eq(identifier.id))
            .order_by_asc(editions::Column::Id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(EditionRow::from))
    }
}
