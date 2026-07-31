//! Edition-to-identifier lookup commands + the `IdentifierRow` specta wrapper.
//!
//! `livtet_data::entities::identifiers::Model` (the SeaORM-generated row
//! type) does not derive `specta::Type`. `IdentifierRow` is a minimal
//! newtype around the columns we want to expose across the IPC boundary,
//! with the derive macros needed for tauri-specta to generate a TS type.
//!
//! An edition can be linked to multiple identifiers (isbn + oclc + lccn +
//! wikidata Q-id, …), so this returns `Vec<IdentifierRow>`, not
//! `Option<…`.

use livtet_core::DbId;
use livtet_core::data::orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// Identifier fields exposed to the webview. Mirrors the columns we
/// want in the detail view's Identifiers tab; add fields here when the
/// UI asks for them, not preemptively.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IdentifierRow {
    pub id: String,
    pub kind: String,
    pub value: String,
}

impl From<livtet_core::data::entities::identifiers::Model> for IdentifierRow {
    fn from(m: livtet_core::data::entities::identifiers::Model) -> Self {
        Self {
            id: m.id.to_string(),
            kind: m.kind,
            value: m.value,
        }
    }
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err, fields(edition_id))]
pub async fn find_identifiers_by_edition(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<Vec<IdentifierRow>, String> {
    let db = state.db.db_conn();
    let edition_id = edition_id
        .parse::<livtet_core::DbId>()
        .map_err(|e| format!("invalid id: {e}"))?;
    let rows = find_identifiers_by_edition_for_test(&db, edition_id).await?;
    Ok(rows)
}

/// Bypasses Tauri's `State<'_, AppState>` so the same code path runs
/// from the production command and from in-memory DB tests.
///
/// `edition_identifiers` has a composite PK on `(edition_id,
/// identifier_id)`. We use a two-query pattern: fetch the junction
/// rows for the edition, then load each identifier by id and map to
/// `IdentifierRow`. We don't `select_only` into a `FromQueryResult`
/// — `IdentifierRow` keeps `id` as `String` (consistent with
/// `EditionRow` / `AuthorWithRole` for the IPC boundary) while the
/// underlying column is `DbId`; the two-query pattern keeps the
/// wrapper uniform.
pub(crate) async fn find_identifiers_by_edition_for_test(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<IdentifierRow>, String> {
    use livtet_core::data::entities::{edition_identifiers, identifiers};

    let junctions = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::EditionId.eq(edition_id))
        .order_by_asc(edition_identifiers::Column::IdentifierId)
        .all(db)
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(junctions.len());
    for j in junctions {
        let identifier = identifiers::Entity::find_by_id(j.identifier_id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        // Junction refers to an identifier that must exist
        // (composite FK). If it's missing the schema is corrupt —
        // surface the error rather than silently dropping the row.
        let identifier = identifier.ok_or_else(|| {
            format!(
                "edition_identifiers row references missing identifier {}",
                j.identifier_id
            )
        })?;
        out.push(IdentifierRow::from(identifier));
    }
    Ok(out)
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
            description: Set(None),
            created_at: Set(now()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn seed_identifier(
        db: &DatabaseConnection,
        kind: &str,
        value: &str,
    ) -> identifiers::Model {
        identifiers::ActiveModel {
            id: Set(DbId::new()),
            kind: Set(kind.into()),
            value: Set(value.into()),
        }
        .insert(db)
        .await
        .unwrap()
    }

    async fn link(db: &DatabaseConnection, edition_id: DbId, identifier_id: DbId) {
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn find_identifiers_by_edition_returns_seeded_rows() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let isbn = seed_identifier(&sea, "isbn", "urn:isbn:9780061120084").await;
        let wikidata = seed_identifier(&sea, "wikidata", "urn:wikidata:Q193359").await;
        link(&sea, edition.id, isbn.id).await;
        link(&sea, edition.id, wikidata.id).await;

        let found = find_identifiers_by_edition_for_test(&sea, edition.id)
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
        let kinds: Vec<&str> = found.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"isbn"));
        assert!(kinds.contains(&"wikidata"));
        let isbn_row = found.iter().find(|i| i.kind == "isbn").unwrap();
        assert_eq!(isbn_row.value, "urn:isbn:9780061120084");
        assert_eq!(isbn_row.id, isbn.id.to_string());
    }

    #[tokio::test]
    async fn find_identifiers_by_edition_returns_empty_when_none_linked() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        // Seed an identifier but don't link it to anything — must not leak.
        let _orphan = seed_identifier(&sea, "isbn", "urn:isbn:0000000000000").await;

        let result = find_identifiers_by_edition_for_test(&sea, DbId::new())
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
