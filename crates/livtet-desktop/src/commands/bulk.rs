use std::path::Path;

use livtet_core::data::entities::{digital_inventory, edition_specific_covers, editions};
use livtet_core::data::orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use livtet_types::DbId;
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::types::AppState;

#[derive(Debug, Clone, Serialize, Type)]
pub struct BulkError {
    pub code: String,
    pub message: String,
}

impl BulkError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeleteSkip {
    pub edition_id: DbId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeleteOutcome {
    pub deleted: i32,
    pub files_removed: i32,
    pub covers_removed: i32,
    pub skipped: Vec<DeleteSkip>,
}

impl DeleteOutcome {
    pub fn empty() -> Self {
        Self {
            deleted: 0,
            files_removed: 0,
            covers_removed: 0,
            skipped: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ExportOutcome {
    pub path: String,
    pub rows: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct TagMutationOutcome {
    pub tag: crate::commands::search::FilterOption,
    pub changed: i32,
}

/// RFC 4180-style field escaping: wrap in quotes when the value contains a
/// comma, quote, CR, or LF, doubling any embedded quotes.
pub fn csv_escape(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn csv_row(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|f| csv_escape(f))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::{DeleteOutcome, DeleteSkip, csv_escape, csv_row};
    use livtet_types::DbId;

    #[test]
    fn csv_escape_quotes_when_needed() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_escape("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn csv_row_joins_with_commas() {
        assert_eq!(csv_row(&["a", "b,c", "d"]), "a,\"b,c\",d");
    }

    #[test]
    fn empty_outcome_is_all_zero() {
        let outcome = DeleteOutcome::empty();
        assert_eq!(
            (
                outcome.deleted,
                outcome.files_removed,
                outcome.covers_removed
            ),
            (0, 0, 0)
        );
        assert!(outcome.skipped.is_empty());
        let _ = DeleteSkip {
            edition_id: DbId::new(),
            reason: "x".into(),
        };
    }
}

struct DeletionPlan {
    edition_id: DbId,
    file_path: Option<String>,
    cover_paths: Vec<String>,
}

async fn collect_deletion_plans(
    db: &livtet_core::data::orm::DatabaseConnection,
    ids: &[DbId],
) -> Result<Vec<DeletionPlan>, BulkError> {
    let mut plans = Vec::with_capacity(ids.len());
    for id in ids {
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(*id))
            .one(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;

        let mut cover_paths = Vec::new();
        if let Some(row) = &inventory
            && let Some(cover) = &row.cover_path
        {
            cover_paths.push(cover.clone());
        }

        let manual = edition_specific_covers::Entity::find()
            .filter(edition_specific_covers::Column::EditionId.eq(*id))
            .all(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        cover_paths.extend(manual.into_iter().map(|m| m.cover_path));

        plans.push(DeletionPlan {
            edition_id: *id,
            file_path: inventory.and_then(|row| row.file_path),
            cover_paths,
        });
    }
    Ok(plans)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_editions(
    edition_ids: Vec<DbId>,
    state: State<'_, AppState>,
) -> Result<DeleteOutcome, BulkError> {
    if edition_ids.is_empty() {
        return Ok(DeleteOutcome::empty());
    }

    let db = state.db.db_conn();
    let plans = collect_deletion_plans(&db, &edition_ids).await?;

    editions::Entity::delete_many()
        .filter(editions::Column::Id.is_in(edition_ids.clone()))
        .exec(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    let mut outcome = DeleteOutcome {
        deleted: edition_ids.len() as i32,
        ..DeleteOutcome::empty()
    };

    for plan in &plans {
        if let Some(path) = &plan.file_path {
            match tokio::fs::remove_file(path).await {
                Ok(()) => outcome.files_removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => outcome.skipped.push(DeleteSkip {
                    edition_id: plan.edition_id,
                    reason: format!("file: {e}"),
                }),
            }
        }
        for path in &plan.cover_paths {
            match tokio::fs::remove_file(path).await {
                Ok(()) => outcome.covers_removed += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => outcome.skipped.push(DeleteSkip {
                    edition_id: plan.edition_id,
                    reason: format!("cover: {e}"),
                }),
            }
            if let Some(parent) = Path::new(path).parent() {
                let _ = tokio::fs::remove_dir(parent).await;
            }
        }
    }

    let guard = state.search_index.read().await;
    if let Some(index) = guard.as_ref() {
        for id in &edition_ids {
            if let Err(e) = index.delete_edition(*id).await {
                outcome.skipped.push(DeleteSkip {
                    edition_id: *id,
                    reason: format!("index: {e}"),
                });
            }
        }
    }

    Ok(outcome)
}

#[tauri::command]
#[specta::specta]
pub async fn export_editions_csv(
    edition_ids: Vec<DbId>,
    path: String,
    state: State<'_, AppState>,
) -> Result<ExportOutcome, BulkError> {
    use livtet_core::data::entities::{
        authors, edition_authors, edition_identifiers, edition_publishers, editions, formats,
        identifiers, languages, publishers, works,
    };

    let db = state.db.db_conn();

    let mut lines: Vec<String> = Vec::with_capacity(edition_ids.len() + 1);
    lines.push(csv_row(&[
        "edition_id",
        "work_id",
        "title",
        "authors",
        "isbn",
        "publisher",
        "language",
        "format",
        "published_date",
    ]));

    for id in &edition_ids {
        let edition = editions::Entity::find_by_id(*id)
            .one(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?
            .ok_or_else(|| BulkError::new("not_found", format!("edition {id}")))?;

        let work_title = works::Entity::find_by_id(edition.work_id)
            .one(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?
            .map(|w| w.title);

        let author_rows = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(*id))
            .all(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut author_names = Vec::new();
        for link in author_rows {
            if let Some(a) = authors::Entity::find_by_id(link.author_id)
                .one(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
            {
                author_names.push(a.name);
            }
        }

        let identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(*id))
            .all(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut isbns = Vec::new();
        for link in identifier_rows {
            if let Some(i) = identifiers::Entity::find_by_id(link.identifier_id)
                .one(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
                && i.kind.eq_ignore_ascii_case("isbn")
            {
                isbns.push(i.value);
            }
        }

        let publisher_rows = edition_publishers::Entity::find()
            .filter(edition_publishers::Column::EditionId.eq(*id))
            .all(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut publisher_names = Vec::new();
        for link in publisher_rows {
            if let Some(p) = publishers::Entity::find_by_id(link.publisher_id)
                .one(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
            {
                publisher_names.push(p.name);
            }
        }

        let language = match edition.language_id {
            Some(lid) => languages::Entity::find_by_id(lid)
                .one(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
                .map(|l| l.name),
            None => None,
        };
        let format = match edition.format_id {
            Some(fid) => formats::Entity::find_by_id(fid)
                .one(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
                .map(|f| f.name),
            None => None,
        };

        let title = edition.title.clone().or(work_title).unwrap_or_default();
        lines.push(csv_row(&[
            &edition.id.to_string(),
            &edition.work_id.to_string(),
            &title,
            &author_names.join("; "),
            &isbns.join("; "),
            &publisher_names.join("; "),
            language.as_deref().unwrap_or(""),
            format.as_deref().unwrap_or(""),
            &edition
                .published_date
                .map(|d| d.to_string())
                .unwrap_or_default(),
        ]));
    }

    let tmp = format!("{path}.tmp");
    let body = lines.join("\n") + "\n";
    tokio::fs::write(&tmp, body.as_bytes())
        .await
        .map_err(|e| BulkError::new("io", e.to_string()))?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(|e| BulkError::new("io", e.to_string()))?;

    Ok(ExportOutcome {
        path,
        rows: edition_ids.len() as i32,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn add_edition_tags(
    edition_ids: Vec<DbId>,
    tag: String,
    state: State<'_, AppState>,
) -> Result<TagMutationOutcome, BulkError> {
    use livtet_core::data::entities::{edition_tags, tags};
    use livtet_types::now_primitive;

    let name = tag.trim().to_string();
    if name.is_empty() {
        return Err(BulkError::new("invalid_input", "tag name is empty"));
    }

    let db = state.db.db_conn();

    let existing = tags::Entity::find()
        .filter(tags::Column::Name.eq(name.clone()))
        .one(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    let tag = match existing {
        Some(row) => row,
        None => {
            let model = tags::ActiveModel {
                id: Set(DbId::new()),
                name: Set(name),
                created_at: Set(now_primitive()),
                updated_at: Set(None),
            };
            model
                .insert(&db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
        }
    };

    let mut changed = 0;
    for id in &edition_ids {
        let already = edition_tags::Entity::find()
            .filter(edition_tags::Column::EditionId.eq(*id))
            .filter(edition_tags::Column::TagId.eq(tag.id))
            .one(&db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        if already.is_some() {
            continue;
        }
        edition_tags::ActiveModel {
            edition_id: Set(*id),
            tag_id: Set(tag.id),
        }
        .insert(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;
        changed += 1;
    }

    if changed > 0 {
        let guard = state.search_index.read().await;
        if let Some(index) = guard.as_ref() {
            index
                .reindex(&db)
                .await
                .map_err(|e| BulkError::new("index", e.to_string()))?;
        }
    }

    Ok(TagMutationOutcome {
        tag: crate::commands::search::FilterOption {
            id: tag.id,
            label: tag.name,
        },
        changed,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn remove_edition_tags(
    edition_ids: Vec<DbId>,
    tag_id: DbId,
    state: State<'_, AppState>,
) -> Result<TagMutationOutcome, BulkError> {
    use livtet_core::data::entities::{edition_tags, tags};

    let db = state.db.db_conn();

    let tag = tags::Entity::find_by_id(tag_id)
        .one(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?
        .ok_or_else(|| BulkError::new("not_found", "tag"))?;

    let result = edition_tags::Entity::delete_many()
        .filter(edition_tags::Column::EditionId.is_in(edition_ids))
        .filter(edition_tags::Column::TagId.eq(tag_id))
        .exec(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    let changed = result.rows_affected as i32;
    if changed > 0 {
        let guard = state.search_index.read().await;
        if let Some(index) = guard.as_ref() {
            index
                .reindex(&db)
                .await
                .map_err(|e| BulkError::new("index", e.to_string()))?;
        }
    }

    Ok(TagMutationOutcome {
        tag: crate::commands::search::FilterOption {
            id: tag.id,
            label: tag.name,
        },
        changed,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn matching_edition_ids(
    query: Option<String>,
    filters: Option<crate::commands::search::EditionFilters>,
    state: State<'_, AppState>,
) -> Result<Vec<DbId>, BulkError> {
    use livtet_core::data::entities::editions;

    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or_else(|| BulkError::new("index", "search index unavailable"))?;

    let db = state.db.db_conn();
    let q = query.as_deref().unwrap_or("");

    let (built, _opts) =
        crate::commands::search::build_filtered(index, &db, q, filters.unwrap_or_default())
            .await
            .map_err(|e| BulkError::new("index", format!("{e:?}")))?;

    let built_query = built
        .build_query(index.index())
        .map_err(|e| BulkError::new("index", e.to_string()))?;

    let work_ids = index
        .matching_work_ids_from_query(&*built_query)
        .await
        .map_err(|e| BulkError::new("index", e.to_string()))?;

    if work_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = editions::Entity::find()
        .filter(editions::Column::WorkId.is_in(work_ids))
        .all(&db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    Ok(rows.into_iter().map(|e| e.id).collect())
}
