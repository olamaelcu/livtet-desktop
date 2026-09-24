use std::path::Path;

use livtet_core::data::entities::{digital_inventory, edition_specific_covers, editions};
use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
use livtet_types::DbId;
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::types::AppState;

#[derive(Debug, Clone, Serialize, Type)]
pub struct CatalogError {
    pub code: String,
    pub message: String,
}

impl CatalogError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
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
        Self { deleted: 0, files_removed: 0, covers_removed: 0, skipped: Vec::new() }
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
    fields.iter().map(|f| csv_escape(f)).collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::{csv_escape, csv_row, DeleteOutcome, DeleteSkip};
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
        assert_eq!((outcome.deleted, outcome.files_removed, outcome.covers_removed), (0, 0, 0));
        assert!(outcome.skipped.is_empty());
        let _ = DeleteSkip { edition_id: DbId::new(), reason: "x".into() };
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
) -> Result<Vec<DeletionPlan>, CatalogError> {
    let mut plans = Vec::with_capacity(ids.len());
    for id in ids {
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(*id))
            .one(db)
            .await
            .map_err(|e| CatalogError::new("database", e.to_string()))?;

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
            .map_err(|e| CatalogError::new("database", e.to_string()))?;
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
) -> Result<DeleteOutcome, CatalogError> {
    if edition_ids.is_empty() {
        return Ok(DeleteOutcome::empty());
    }

    let db = state.db.db_conn();
    let plans = collect_deletion_plans(&db, &edition_ids).await?;

    editions::Entity::delete_many()
        .filter(editions::Column::Id.is_in(edition_ids.clone()))
        .exec(&db)
        .await
        .map_err(|e| CatalogError::new("database", e.to_string()))?;

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
