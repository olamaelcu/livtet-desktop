//! Editing the identifiers attached to one edition.
//!
//! The drawer's Identifiers section lets a reader correct a mistyped ISBN or
//! drop an entry a metadata source invented. Both writes act on the link in
//! `edition_identifiers`, never on the identifier row directly: `identifiers.value`
//! is unique across the whole catalog, so the row is shared by every edition
//! carrying that value. An edit therefore unlinks and re-links, and the shared
//! row is only rewritten (created or dropped) once nothing else points at it —
//! otherwise a kind change here would silently change another book's identifier.
//!
//! Identifiers feed the search index (`identifier_values` / `identifier_kinds`
//! in every edition document), so both commands rebuild the one document they
//! touched. That step is best effort: the catalog write is the source of truth,
//! and a missing index must not report a stored edit as failed.

use tauri::State;

use livtet_core::data::entities::{edition_identifiers, identifiers};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, Set, TransactionTrait,
};
use livtet_core::search::SearchIndex;
use livtet_types::DbId;

use crate::error::CatalogError;
use crate::types::AppState;

/// Remove the identifier `(kind, value)` from one edition.
///
/// The link goes; the identifier row follows once no edition links it anymore,
/// so a shared identifier survives on the books that still carry it.
#[tauri::command]
#[specta::specta]
pub async fn remove_edition_identifier(
    edition_id: String,
    kind: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<(), CatalogError> {
    let id = parse_edition_id(&edition_id)?;
    let db = state.db.db_conn();
    let index = state.search_index.read().await;
    remove_identifier(&db, index.as_deref(), id, &kind, &value).await
}

/// Replace one identifier of an edition with `(new_kind, new_value)`.
///
/// Both halves are editable because a source can get either wrong: an entry
/// filed as `other` that is really an OCLC number changes kind, not value.
#[tauri::command]
#[specta::specta]
pub async fn update_edition_identifier(
    edition_id: String,
    kind: String,
    value: String,
    new_kind: String,
    new_value: String,
    state: State<'_, AppState>,
) -> Result<(), CatalogError> {
    let id = parse_edition_id(&edition_id)?;
    let db = state.db.db_conn();
    let index = state.search_index.read().await;
    update_identifier(
        &db,
        index.as_deref(),
        id,
        (kind.as_str(), value.as_str()),
        (new_kind.as_str(), new_value.as_str()),
    )
    .await
}

fn parse_edition_id(edition_id: &str) -> Result<DbId, CatalogError> {
    edition_id
        .parse::<DbId>()
        .map_err(|_| CatalogError::InvalidId {
            id: edition_id.to_string(),
        })
}

/// Detach `(kind, value)` from the edition. Kept free of Tauri state so it can
/// be unit-tested against a `TestDb`.
pub(crate) async fn remove_identifier(
    db: &DatabaseConnection,
    index: Option<&SearchIndex>,
    edition_id: DbId,
    kind: &str,
    value: &str,
) -> Result<(), CatalogError> {
    let identifier_id = linked_identifier(db, edition_id, kind, value).await?;

    let txn = db.begin().await.map_err(CatalogError::database)?;
    detach(&txn, edition_id, identifier_id).await?;
    txn.commit().await.map_err(CatalogError::database)?;

    reindex(index, db, edition_id).await;
    Ok(())
}

/// Re-point the edition's `(kind, value)` entry at `(new_kind, new_value)`.
///
/// Runs in one transaction: the old link only disappears if the new one lands,
/// so a rejected edit leaves the edition exactly as it was.
pub(crate) async fn update_identifier(
    db: &DatabaseConnection,
    index: Option<&SearchIndex>,
    edition_id: DbId,
    (kind, value): (&str, &str),
    (new_kind, new_value): (&str, &str),
) -> Result<(), CatalogError> {
    let new_kind = new_kind.trim();
    let new_value = new_value.trim();
    if new_kind.is_empty() || new_value.is_empty() {
        return Err(CatalogError::invalid_identifier(
            "an identifier needs both a kind and a value",
        ));
    }

    let identifier_id = linked_identifier(db, edition_id, kind, value).await?;
    if (kind, value) == (new_kind, new_value) {
        return Ok(());
    }

    let txn = db.begin().await.map_err(CatalogError::database)?;
    detach(&txn, edition_id, identifier_id).await?;
    attach(&txn, edition_id, new_kind, new_value).await?;
    txn.commit().await.map_err(CatalogError::database)?;

    reindex(index, db, edition_id).await;
    Ok(())
}

/// The identifier row behind `(kind, value)`, once the edition is proven to
/// link it. A row that exists but is not linked is not this edition's to edit.
async fn linked_identifier(
    db: &DatabaseConnection,
    edition_id: DbId,
    kind: &str,
    value: &str,
) -> Result<DbId, CatalogError> {
    let row = identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(value))
        .one(db)
        .await
        .map_err(CatalogError::database)?;

    if let Some(row) = row
        && row.kind == kind
        && edition_identifiers::Entity::find_by_id((edition_id, row.id))
            .one(db)
            .await
            .map_err(CatalogError::database)?
            .is_some()
    {
        return Ok(row.id);
    }

    Err(CatalogError::IdentifierNotFound {
        edition_id: edition_id.to_string(),
        kind: kind.to_string(),
        value: value.to_string(),
    })
}

/// Drop the link, then the identifier row once nothing else points at it.
async fn detach<C: ConnectionTrait>(
    conn: &C,
    edition_id: DbId,
    identifier_id: DbId,
) -> Result<(), CatalogError> {
    edition_identifiers::Entity::delete_by_id((edition_id, identifier_id))
        .exec(conn)
        .await
        .map_err(CatalogError::database)?;

    let links = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::IdentifierId.eq(identifier_id))
        .count(conn)
        .await
        .map_err(CatalogError::database)?;
    if links == 0 {
        identifiers::Entity::delete_by_id(identifier_id)
            .exec(conn)
            .await
            .map_err(CatalogError::database)?;
    }
    Ok(())
}

/// Link `(kind, value)` to the edition, reusing an identifier row that already
/// carries the value.
///
/// `kind` lives on the shared `identifiers` row, so a value another book stores
/// under a different kind is a conflict rather than a silent rewrite of that
/// book's identifier.
async fn attach<C: ConnectionTrait>(
    conn: &C,
    edition_id: DbId,
    kind: &str,
    value: &str,
) -> Result<(), CatalogError> {
    let identifier_id = match identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(value))
        .one(conn)
        .await
        .map_err(CatalogError::database)?
    {
        Some(existing) if existing.kind == kind => existing.id,
        Some(existing) => {
            return Err(CatalogError::IdentifierConflict {
                value: value.to_string(),
                existing_kind: existing.kind,
                requested_kind: kind.to_string(),
            });
        }
        None => {
            identifiers::ActiveModel {
                id: Set(DbId::new()),
                value: Set(value.to_string()),
                kind: Set(kind.to_string()),
            }
            .insert(conn)
            .await
            .map_err(CatalogError::database)?
            .id
        }
    };

    let already_linked = edition_identifiers::Entity::find_by_id((edition_id, identifier_id))
        .one(conn)
        .await
        .map_err(CatalogError::database)?
        .is_some();
    if !already_linked {
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(conn)
        .await
        .map_err(CatalogError::database)?;
    }
    Ok(())
}

/// Rebuild the edition's search document. Best effort, see the module docs.
async fn reindex(index: Option<&SearchIndex>, db: &DatabaseConnection, edition_id: DbId) {
    let Some(index) = index else {
        return;
    };
    if let Err(error) = index.add_edition(db, edition_id).await {
        tracing::warn!(
            %error,
            %edition_id,
            "could not reindex the edition after an identifier edit"
        );
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use livtet_core::data::entities::{editions, works};
    use livtet_core::data::orm::PaginatorTrait;
    use livtet_core::data::{Kind, TestDb};
    use livtet_types::now_primitive;

    const ISBN: (&str, &str) = ("isbn", "urn:isbn:9780306406157");
    const OCLC: (&str, &str) = ("oclc", "urn:oclc:12345");

    /// One empty edition under its own work.
    async fn seed_edition(db: &DatabaseConnection) -> DbId {
        let now = now_primitive();
        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("The Book".to_string()),
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
            title: Set(Some("The Book".to_string())),
            published_date: Set(None),
            format_id: Set(None),
            language_id: Set(None),
            notes: Set(None),
            description: Set(None),
            format_metadata: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");
        edition_id
    }

    /// One `identifiers` row, linked to none.
    async fn seed_identifier(db: &DatabaseConnection, (kind, value): (&str, &str)) -> DbId {
        let identifier_id = DbId::new();
        identifiers::ActiveModel {
            id: Set(identifier_id),
            value: Set(value.to_string()),
            kind: Set(kind.to_string()),
        }
        .insert(db)
        .await
        .expect("identifier");
        identifier_id
    }

    async fn link(db: &DatabaseConnection, edition_id: DbId, identifier_id: DbId) {
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(db)
        .await
        .expect("edition identifier");
    }

    async fn linked_values(db: &DatabaseConnection, edition_id: DbId) -> Vec<(String, String)> {
        let links = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(db)
            .await
            .expect("links");
        let ids: Vec<DbId> = links.iter().map(|link| link.identifier_id).collect();
        if ids.is_empty() {
            return Vec::new();
        }
        let mut rows: Vec<(String, String)> = identifiers::Entity::find()
            .filter(identifiers::Column::Id.is_in(ids))
            .all(db)
            .await
            .expect("identifiers")
            .into_iter()
            .map(|row| (row.kind, row.value))
            .collect();
        rows.sort();
        rows
    }

    async fn identifier_rows(db: &DatabaseConnection) -> i64 {
        identifiers::Entity::find().count(db).await.expect("count") as i64
    }

    fn pair((kind, value): (&str, &str)) -> (String, String) {
        (kind.to_string(), value.to_string())
    }

    #[tokio::test]
    async fn removing_drops_the_link_and_the_orphaned_identifier() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;

        remove_identifier(&db, None, edition_id, ISBN.0, ISBN.1)
            .await
            .expect("removed");

        assert!(linked_values(&db, edition_id).await.is_empty());
        assert_eq!(identifier_rows(&db).await, 0, "orphan row is gone");
    }

    #[tokio::test]
    async fn removing_keeps_an_identifier_another_edition_still_carries() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        let neighbour = seed_edition(&db).await;
        let identifier_id = seed_identifier(&db, ISBN).await;
        link(&db, edition_id, identifier_id).await;
        link(&db, neighbour, identifier_id).await;

        remove_identifier(&db, None, edition_id, ISBN.0, ISBN.1)
            .await
            .expect("removed");

        assert!(linked_values(&db, edition_id).await.is_empty());
        assert_eq!(
            linked_values(&db, neighbour).await,
            vec![pair(ISBN)],
            "the other edition keeps its identifier"
        );
        assert_eq!(
            identifier_rows(&db).await,
            1,
            "no orphan cleanup needed yet"
        );
    }

    #[tokio::test]
    async fn removing_an_identifier_the_edition_does_not_carry_is_rejected() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;
        let stranger = seed_edition(&db).await;

        // Right value, wrong kind: the pair is what identifies the entry.
        let error = remove_identifier(&db, None, edition_id, "oclc", ISBN.1)
            .await
            .expect_err("wrong kind");
        assert!(matches!(error, CatalogError::IdentifierNotFound { .. }));

        let error = remove_identifier(&db, None, edition_id, "isbn", "urn:isbn:0000000000000")
            .await
            .expect_err("unknown value");
        assert!(matches!(error, CatalogError::IdentifierNotFound { .. }));

        // A row nobody linked is not an edition's to remove.
        let error = remove_identifier(&db, None, stranger, ISBN.0, ISBN.1)
            .await
            .expect_err("not linked");
        assert!(matches!(error, CatalogError::IdentifierNotFound { .. }));

        assert_eq!(
            linked_values(&db, edition_id).await,
            vec![pair(ISBN)],
            "nothing was removed"
        );
        assert_eq!(identifier_rows(&db).await, 1);
    }

    #[tokio::test]
    async fn updating_the_value_replaces_the_stored_identifier() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;

        update_identifier(
            &db,
            None,
            edition_id,
            ISBN,
            ("isbn", "urn:isbn:9780802124675"),
        )
        .await
        .expect("updated");

        assert_eq!(
            linked_values(&db, edition_id).await,
            vec![pair(("isbn", "urn:isbn:9780802124675"))]
        );
        assert_eq!(identifier_rows(&db).await, 1, "the old row was dropped");
    }

    #[tokio::test]
    async fn updating_the_kind_rewrites_the_identifier() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;

        update_identifier(&db, None, edition_id, ISBN, ("oclc", ISBN.1))
            .await
            .expect("updated");

        assert_eq!(
            linked_values(&db, edition_id).await,
            vec![pair(("oclc", ISBN.1))]
        );
    }

    #[tokio::test]
    async fn updating_to_an_unchanged_pair_is_a_no_op() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;

        update_identifier(&db, None, edition_id, ISBN, (" isbn ", ISBN.1))
            .await
            .expect("no-op");

        assert_eq!(linked_values(&db, edition_id).await, vec![pair(ISBN)]);
    }

    #[tokio::test]
    async fn updating_rejects_an_empty_kind_or_value() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;

        for (new_kind, new_value) in [("  ", "urn:isbn:1"), ("isbn", "   ")] {
            let error = update_identifier(&db, None, edition_id, ISBN, (new_kind, new_value))
                .await
                .expect_err("blank field");
            assert!(matches!(error, CatalogError::InvalidIdentifier { .. }));
        }

        assert_eq!(
            linked_values(&db, edition_id).await,
            vec![pair(ISBN)],
            "the entry survived both rejections"
        );
    }

    #[tokio::test]
    async fn updating_onto_a_value_another_edition_holds_links_the_shared_row() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        let neighbour = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;
        let shared = seed_identifier(&db, OCLC).await;
        link(&db, neighbour, shared).await;

        update_identifier(&db, None, edition_id, ISBN, OCLC)
            .await
            .expect("updated");

        assert_eq!(linked_values(&db, edition_id).await, vec![pair(OCLC)]);
        assert_eq!(
            linked_values(&db, neighbour).await,
            vec![pair(OCLC)],
            "the shared row kept its link"
        );
        assert_eq!(identifier_rows(&db).await, 1, "the ISBN row was dropped");
    }

    #[tokio::test]
    async fn updating_onto_a_value_stored_under_another_kind_is_a_conflict() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition_id = seed_edition(&db).await;
        let neighbour = seed_edition(&db).await;
        link(&db, edition_id, seed_identifier(&db, ISBN).await).await;
        link(&db, neighbour, seed_identifier(&db, OCLC).await).await;

        let error = update_identifier(&db, None, edition_id, ISBN, ("lccn", OCLC.1))
            .await
            .expect_err("kind clash");
        assert!(
            matches!(error, CatalogError::IdentifierConflict { .. }),
            "got {error:?}"
        );

        // The transaction rolled back: the edition still carries its ISBN.
        assert_eq!(linked_values(&db, edition_id).await, vec![pair(ISBN)]);
        assert_eq!(linked_values(&db, neighbour).await, vec![pair(OCLC)]);
        assert_eq!(identifier_rows(&db).await, 2);
    }
}
