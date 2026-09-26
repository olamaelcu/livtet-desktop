//! `get_edition_detail` — assemble one edition's full catalog record for the
//! library's edition-detail drawer: edition/work metadata, contributors with
//! roles, publishers, identifiers, and the backing digital inventory row.
//!
//! `get_edition_covers` — resolve the extracted cover path for a page of
//! editions in one indexed query, so the library grid can render covers without
//! paying for the full detail joins per card.
//!
//! Both are read-only and fail-closed: an unparseable id is rejected before any
//! query, and a missing edition returns `None` rather than an error.

use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use tauri::State;

use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers, editions,
    formats, identifiers, languages, publishers,
};
use livtet_core::data::orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use livtet_types::DbId;

use crate::error::CatalogError;
use crate::roles::role_label;
use crate::types::AppState;

/// A contributor attached to an edition, with their role code (e.g. `aut`)
/// and its human-readable label (e.g. `Author`).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct Contributor {
    pub name: String,
    pub role: String,
    pub role_label: String,
}

/// One identifier attached to an edition (e.g. `kind = "isbn"`,
/// `value = "urn:isbn:9780306406157"`).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct IdentifierRef {
    pub kind: String,
    pub value: String,
}

/// Whether the library file behind an edition is reachable. `Missing` means
/// the stored path (or its link target) no longer exists on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Ok,
    Missing,
}

/// The digital file backing an edition, when one is on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EditionFile {
    pub file_path: Option<String>,
    pub file_format: Option<String>,
    /// Byte size as a JS `number` (specta cannot export `i64`); exact to 2^53.
    pub file_size_bytes: Option<f64>,
    /// Absolute path to the extracted cover image, if any.
    pub cover_path: Option<String>,
    pub blurhash: Option<String>,
    pub dominant_color: Option<String>,
    /// Reachability of the stored path. `None` when the edition has no file.
    pub file_status: Option<FileStatus>,
}

/// Full read-only detail for one edition.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EditionDetail {
    pub id: String,
    pub work_id: String,
    pub title: Option<String>,
    /// `YYYY-MM-DD`, when the edition carries a publication date.
    pub published_date: Option<String>,
    pub format: Option<String>,
    pub language_code: Option<String>,
    /// English display name from `languages.name` (e.g. `"English"`).
    pub language_name: Option<String>,
    pub notes: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<Contributor>,
    pub publishers: Vec<String>,
    pub identifiers: Vec<IdentifierRef>,
    pub file: Option<EditionFile>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// One edition's extracted cover path, as stored on its digital inventory row.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct EditionCover {
    pub edition_id: String,
    /// Absolute path to the cover image on disk.
    pub cover_path: String,
}

/// Fetch one edition's detail. `Ok(None)` when no edition with this id exists.
#[tauri::command]
#[specta::specta]
pub async fn get_edition_detail(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<EditionDetail>, CatalogError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| CatalogError::InvalidId { id: edition_id })?;
    fetch_edition_detail(&state.db.db_conn(), id).await
}

/// Resolve the cover path for a batch of editions. Editions without a cover
/// (or without a digital inventory row) are omitted; an empty input yields an
/// empty result.
#[tauri::command]
#[specta::specta]
pub async fn get_edition_covers(
    edition_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<Vec<EditionCover>, CatalogError> {
    let ids = edition_ids
        .iter()
        .map(|id| {
            id.parse::<DbId>()
                .map_err(|_| CatalogError::InvalidId { id: id.clone() })
        })
        .collect::<Result<Vec<_>, _>>()?;
    fetch_edition_covers(&state.db.db_conn(), &ids).await
}

/// Query cover paths for a set of editions in one indexed lookup. Kept free of
/// Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn fetch_edition_covers(
    db: &DatabaseConnection,
    edition_ids: &[DbId],
) -> Result<Vec<EditionCover>, CatalogError> {
    if edition_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.is_in(edition_ids.iter().copied()))
        .filter(digital_inventory::Column::CoverPath.is_not_null())
        .all(db)
        .await
        .map_err(CatalogError::database)?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            row.cover_path.map(|cover_path| EditionCover {
                edition_id: row.edition_id.to_string(),
                cover_path,
            })
        })
        .collect())
}

/// Query and assemble the detail row. Kept free of Tauri state so it can be
/// unit-tested against a `TestDb`.
pub(crate) async fn fetch_edition_detail(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<EditionDetail>, CatalogError> {
    let Some(model) = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await
        .map_err(CatalogError::database)?
    else {
        return Ok(None);
    };

    assemble_edition_detail(db, model).await.map(Some)
}

async fn assemble_edition_detail(
    db: &DatabaseConnection,
    model: editions::Model,
) -> Result<EditionDetail, CatalogError> {
    let format = match model.format_id {
        Some(id) => formats::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(CatalogError::database)?
            .map(|format| format.name),
        None => None,
    };
    let language = match model.language_id {
        Some(id) => languages::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(CatalogError::database)?,
        None => None,
    };
    let language_code = language.as_ref().map(|language| language.code.clone());
    let language_name = language.as_ref().map(|language| language.name.clone());

    Ok(EditionDetail {
        id: model.id.to_string(),
        work_id: model.work_id.to_string(),
        title: model.title,
        published_date: model.published_date.map(format_date),
        format,
        language_code,
        language_name,
        notes: model.notes,
        description: model.description,
        authors: contributors_for_edition(db, model.id).await?,
        publishers: publishers_for_edition(db, model.id).await?,
        identifiers: identifiers_for_edition(db, model.id).await?,
        file: file_for_edition(db, model.id).await?,
        created_at: format_timestamp(model.created_at),
        updated_at: model.updated_at.map(format_timestamp),
    })
}

async fn contributors_for_edition(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<Contributor>, CatalogError> {
    let links = edition_authors::Entity::find()
        .filter(edition_authors::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(CatalogError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.author_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let names: HashMap<DbId, String> = authors::Entity::find()
        .filter(authors::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(CatalogError::database)?
        .into_iter()
        .map(|author| (author.id, author.name))
        .collect();

    Ok(links
        .into_iter()
        .filter_map(|link| {
            names.get(&link.author_id).map(|name| Contributor {
                name: name.clone(),
                role_label: role_label(&link.role),
                role: link.role,
            })
        })
        .collect())
}

async fn publishers_for_edition(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<String>, CatalogError> {
    let links = edition_publishers::Entity::find()
        .filter(edition_publishers::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(CatalogError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.publisher_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(publishers::Entity::find()
        .filter(publishers::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(CatalogError::database)?
        .into_iter()
        .map(|publisher| publisher.name)
        .collect())
}

async fn identifiers_for_edition(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<IdentifierRef>, CatalogError> {
    let links = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(CatalogError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.identifier_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(identifiers::Entity::find()
        .filter(identifiers::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(CatalogError::database)?
        .into_iter()
        .map(|identifier| IdentifierRef {
            kind: identifier.kind,
            value: identifier.value,
        })
        .collect())
}

/// Reachability of a stored library path. `None` when the edition has no
/// file; `exists()` follows symlinks, so a dangling link reads `Missing`.
fn file_status_for(file_path: Option<&str>) -> Option<FileStatus> {
    file_path.map(|path| {
        if std::path::Path::new(path).exists() {
            FileStatus::Ok
        } else {
            FileStatus::Missing
        }
    })
}

async fn file_for_edition(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<EditionFile>, CatalogError> {
    Ok(digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await
        .map_err(CatalogError::database)?
        .map(|inventory| EditionFile {
            file_status: file_status_for(inventory.file_path.as_deref()),
            file_path: inventory.file_path,
            file_format: inventory.file_format,
            file_size_bytes: inventory.file_size_bytes.map(|bytes| bytes as f64),
            cover_path: inventory.cover_path,
            blurhash: inventory.blurhash,
            dominant_color: inventory.dominant_color,
        }))
}

fn format_date(date: time::Date) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

fn format_timestamp(timestamp: time::PrimitiveDateTime) -> String {
    use time::format_description::well_known::Rfc3339;
    timestamp
        .assume_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::entities::works;
    use livtet_core::data::orm::{ActiveModelTrait, Set};
    use livtet_core::data::{Kind, TestDb};
    use livtet_types::now_primitive;

    struct Seed {
        work_id: DbId,
        edition_id: DbId,
    }

    async fn seed_edition(db: &DatabaseConnection) -> Seed {
        let now = now_primitive();
        let format_id = DbId::new();
        let language_id = DbId::new();

        formats::ActiveModel {
            id: Set(format_id),
            name: Set("EPUB".to_string()),
            metadata_schema: Set(serde_json::Value::Null),
            progress_unit: Set(None),
        }
        .insert(db)
        .await
        .expect("format");

        languages::ActiveModel {
            id: Set(language_id),
            name: Set("English".to_string()),
            code: Set("eng".to_string()),
            flag_emoji: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("language");

        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("The Book".to_string()),
            description: Set(Some("A work".to_string())),
            sort_title: Set(None),
            series_type: Set(None),
            language_id: Set(Some(language_id)),
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
            published_date: Set(Some(
                time::Date::from_calendar_date(2001, time::Month::September, 11).expect("date"),
            )),
            format_id: Set(Some(format_id)),
            language_id: Set(Some(language_id)),
            notes: Set(Some("A note".to_string())),
            description: Set(Some("An edition".to_string())),
            format_metadata: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");

        let author_id = DbId::new();
        authors::ActiveModel {
            id: Set(author_id),
            name: Set("Ada Lovelace".to_string()),
        }
        .insert(db)
        .await
        .expect("author");
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set("aut".to_string()),
        }
        .insert(db)
        .await
        .expect("edition author");

        let publisher_id = DbId::new();
        publishers::ActiveModel {
            id: Set(publisher_id),
            name: Set("Acme Press".to_string()),
            website: Set(None),
            logo_url: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("publisher");
        edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
        }
        .insert(db)
        .await
        .expect("edition publisher");

        let identifier_id = DbId::new();
        identifiers::ActiveModel {
            id: Set(identifier_id),
            value: Set("urn:isbn:9780306406157".to_string()),
            kind: Set("isbn".to_string()),
        }
        .insert(db)
        .await
        .expect("identifier");
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(db)
        .await
        .expect("edition identifier");

        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(Some("/books/the-book.epub".to_string())),
            cover_path: Set(Some("/covers/x/cover.jpg".to_string())),
            blurhash: Set(None),
            dominant_color: Set(None),
            file_hash: Set(Some("deadbeef".to_string())),
            file_size_bytes: Set(Some(1024)),
            file_format: Set(Some("epub".to_string())),
            notes: Set(None),
            added_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("digital inventory");

        Seed {
            work_id,
            edition_id,
        }
    }

    #[tokio::test]
    async fn assembles_full_edition_detail() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let seed = seed_edition(&db).await;

        let detail = fetch_edition_detail(&db, seed.edition_id)
            .await
            .expect("query ok")
            .expect("edition present");

        assert_eq!(detail.id, seed.edition_id.to_string());
        assert_eq!(detail.work_id, seed.work_id.to_string());
        assert_eq!(detail.title.as_deref(), Some("The Book"));
        assert_eq!(detail.published_date.as_deref(), Some("2001-09-11"));
        assert_eq!(detail.format.as_deref(), Some("EPUB"));
        assert_eq!(detail.language_code.as_deref(), Some("eng"));
        assert_eq!(detail.language_name.as_deref(), Some("English"));
        assert_eq!(detail.notes.as_deref(), Some("A note"));
        assert_eq!(
            detail.authors,
            vec![Contributor {
                name: "Ada Lovelace".to_string(),
                role: "aut".to_string(),
                role_label: "Author".to_string(),
            }]
        );
        assert_eq!(detail.publishers, vec!["Acme Press".to_string()]);
        assert_eq!(
            detail.identifiers,
            vec![IdentifierRef {
                kind: "isbn".to_string(),
                value: "urn:isbn:9780306406157".to_string(),
            }]
        );

        let file = detail.file.expect("file present");
        assert_eq!(file.file_format.as_deref(), Some("epub"));
        assert_eq!(file.file_size_bytes, Some(1024.0));
        assert_eq!(file.cover_path.as_deref(), Some("/covers/x/cover.jpg"));
    }

    #[tokio::test]
    async fn missing_edition_returns_none() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let detail = fetch_edition_detail(&db, DbId::new())
            .await
            .expect("query ok");
        assert!(detail.is_none());
    }

    #[tokio::test]
    async fn covers_returned_only_for_editions_with_a_cover() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let seed = seed_edition(&db).await;

        let covers = fetch_edition_covers(&db, &[seed.edition_id])
            .await
            .expect("query ok");
        assert_eq!(
            covers,
            vec![EditionCover {
                edition_id: seed.edition_id.to_string(),
                cover_path: "/covers/x/cover.jpg".to_string(),
            }]
        );

        assert!(
            fetch_edition_covers(&db, &[DbId::new()])
                .await
                .expect("query ok")
                .is_empty()
        );
        assert!(
            fetch_edition_covers(&db, &[])
                .await
                .expect("query ok")
                .is_empty()
        );

        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(seed.edition_id))
            .one(&db)
            .await
            .expect("query ok")
            .expect("inventory row");
        let mut model: digital_inventory::ActiveModel = inventory.into();
        model.cover_path = Set(None);
        model.update(&db).await.expect("update cover_path");

        assert!(
            fetch_edition_covers(&db, &[seed.edition_id])
                .await
                .expect("query ok")
                .is_empty()
        );
    }

    async fn set_inventory_file_path(
        db: &DatabaseConnection,
        edition_id: DbId,
        path: Option<String>,
    ) {
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(db)
            .await
            .expect("query ok")
            .expect("inventory row");
        let mut model: digital_inventory::ActiveModel = inventory.into();
        model.file_path = Set(path);
        model.update(db).await.expect("update file_path");
    }

    async fn detail_file(db: &DatabaseConnection, edition_id: DbId) -> EditionFile {
        fetch_edition_detail(db, edition_id)
            .await
            .expect("query ok")
            .expect("edition present")
            .file
            .expect("file present")
    }

    #[tokio::test]
    async fn file_status_reflects_target_presence() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let seed = seed_edition(&db).await;

        // The seeded path exists nowhere on disk.
        assert_eq!(
            detail_file(&db, seed.edition_id).await.file_status,
            Some(FileStatus::Missing)
        );

        // A real file behind the path reads reachable.
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dir.path().join("the-book.epub");
        std::fs::write(&real, b"book bytes").expect("write temp book");
        set_inventory_file_path(
            &db,
            seed.edition_id,
            Some(real.to_string_lossy().into_owned()),
        )
        .await;
        assert_eq!(
            detail_file(&db, seed.edition_id).await.file_status,
            Some(FileStatus::Ok)
        );

        // No stored path means no status at all.
        set_inventory_file_path(&db, seed.edition_id, None).await;
        assert_eq!(detail_file(&db, seed.edition_id).await.file_status, None);
    }
}
