//! Edition-to-author lookup commands + the `AuthorWithRole` specta wrapper.
//!
//! `livtet_data::entities::authors::Model` (the SeaORM-generated row
//! type) does not derive `specta::Type`. `AuthorWithRole` is a minimal
//! newtype around the columns we want to expose across the IPC
//! boundary, with the derive macros needed for tauri-specta to
//! generate a TS type.

use livtet_core::DbId;
use livtet_core::data::orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

/// Author fields exposed to the webview. Mirrors the columns we
/// want in the detail view's Authors tab; add fields here when the
/// UI asks for them, not preemptively.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthorWithRole {
    pub id: String,
    pub name: String,
    pub role: String,
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err, fields(edition_id))]
pub async fn find_authors_by_edition(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<Vec<AuthorWithRole>, String> {
    let db = state.db.db_conn();
    let edition_id = edition_id
        .parse::<livtet_core::DbId>()
        .map_err(|e| format!("invalid id: {e}"))?;
    let rows = find_authors_by_edition_for_test(&db, edition_id).await?;
    Ok(rows)
}

/// Bypasses Tauri's `State<'_, AppState>` so the same code path runs
/// from the production command and from in-memory DB tests.
///
/// `edition_authors` has a composite PK on `(edition_id, author_id,
/// role)`, so the same `(edition_id, author_id)` pair can appear with
/// different roles (e.g. "translator" + "editor"). We use a two-query
/// pattern: fetch the junction rows for the edition, then load each
/// author by id and map to `AuthorWithRole`. We intentionally do NOT
/// do a JOIN with `select_only` into a `FromQueryResult` model —
/// that would force `AuthorWithRole` to type every column identically
/// (DbId vs String), which makes the wrapper less reusable. Two
/// queries against the in-memory DB are cheap and keep the shape
/// obvious.
pub(crate) async fn find_authors_by_edition_for_test(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<AuthorWithRole>, String> {
    use livtet_core::data::entities::{authors, edition_authors};

    let junctions = edition_authors::Entity::find()
        .filter(edition_authors::Column::EditionId.eq(edition_id))
        .order_by_asc(edition_authors::Column::AuthorId)
        .all(db)
        .await
        .map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(junctions.len());
    for j in junctions {
        let author = authors::Entity::find_by_id(j.author_id)
            .one(db)
            .await
            .map_err(|e| e.to_string())?;
        // Junction refers to an author that must exist (composite FK).
        // If it's missing the schema is corrupt — surface the error.
        let author = author.ok_or_else(|| {
            format!(
                "edition_authors row references missing author {}",
                j.author_id
            )
        })?;
        out.push(AuthorWithRole {
            id: author.id.to_string(),
            name: author.name,
            role: j.role,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::DbId;
    use livtet_core::data::TestDb;
    use livtet_core::data::entities::{authors, edition_authors, editions, works};
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

    async fn seed_author(db: &DatabaseConnection, name: &str) -> authors::Model {
        authors::ActiveModel {
            id: Set(DbId::new()),
            name: Set(name.into()),
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn find_authors_by_edition_returns_seeded_rows() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let author = seed_author(&sea, "Ursula K. Le Guin").await;
        edition_authors::ActiveModel {
            edition_id: Set(edition.id),
            author_id: Set(author.id),
            role: Set("author".into()),
        }
        .insert(&sea)
        .await
        .unwrap();

        let found = find_authors_by_edition_for_test(&sea, edition.id)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, author.id.to_string());
        assert_eq!(found[0].name, "Ursula K. Le Guin");
        assert_eq!(found[0].role, "author");
    }

    #[tokio::test]
    async fn find_authors_by_edition_returns_empty_when_no_authors() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let result = find_authors_by_edition_for_test(&sea, DbId::new())
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    /// Schema-invariant guard: `edition_authors` has a composite
    /// primary key on `(edition_id, author_id, role)`. So the same
    /// `(edition_id, author_id)` pair can appear with different roles
    /// (e.g. an author who is both "translator" and "editor"), but
    /// the same `(edition_id, author_id, role)` triple cannot appear
    /// twice. This test pins the composite PK uniqueness on the
    /// full triple so a future migration that drops it would fail
    /// loudly.
    #[tokio::test]
    async fn duplicate_junction_triple_is_rejected() {
        let test_db = TestDb::new(None).await.unwrap();
        let sea = test_db.state().db_conn();
        let work = seed_work(&sea).await;
        let edition = seed_edition(&sea, work.id).await;
        let author = seed_author(&sea, "Ursula K. Le Guin").await;

        edition_authors::ActiveModel {
            edition_id: Set(edition.id),
            author_id: Set(author.id),
            role: Set("translator".into()),
        }
        .insert(&sea)
        .await
        .unwrap();

        // Same (edition_id, author_id, role) triple — must fail.
        let second = edition_authors::ActiveModel {
            edition_id: Set(edition.id),
            author_id: Set(author.id),
            role: Set("translator".into()),
        }
        .insert(&sea)
        .await;
        assert!(
            second.is_err(),
            "junction must reject duplicate (edition_id, author_id, role) triple"
        );
    }
}
