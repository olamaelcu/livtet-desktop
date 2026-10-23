use camino::Utf8Path;

use livtet_core::data::entities::{digital_inventory, edition_specific_covers, editions};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use livtet_core::search::SearchIndex;
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
    let guard = state.search_index.read().await;
    delete_editions_inner(&db, guard.as_ref(), &edition_ids).await
}

/// Delete editions and their files, without Tauri state so tests can drive
/// it against a [`TestDb`]. A missing index skips index cleanup.
///
/// [`TestDb`]: livtet_core::data::TestDb
pub(crate) async fn delete_editions_inner(
    db: &DatabaseConnection,
    index: Option<&SearchIndex>,
    edition_ids: &[DbId],
) -> Result<DeleteOutcome, BulkError> {
    let plans = collect_deletion_plans(db, edition_ids).await?;

    editions::Entity::delete_many()
        .filter(editions::Column::Id.is_in(edition_ids.to_vec()))
        .exec(db)
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
            if let Some(parent) = Utf8Path::new(path).parent() {
                let _ = tokio::fs::remove_dir(parent).await;
            }
        }
    }

    if let Some(index) = index {
        for id in edition_ids {
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
    let db = state.db.db_conn();
    export_editions_csv_inner(&db, &edition_ids, path).await
}

/// Render editions as CSV and write `path` atomically, without Tauri state.
pub(crate) async fn export_editions_csv_inner(
    db: &DatabaseConnection,
    edition_ids: &[DbId],
    path: String,
) -> Result<ExportOutcome, BulkError> {
    use livtet_core::data::entities::{
        authors, edition_authors, edition_identifiers, edition_publishers, editions, formats,
        identifiers, languages, publishers, works,
    };

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

    for id in edition_ids {
        let edition = editions::Entity::find_by_id(*id)
            .one(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?
            .ok_or_else(|| BulkError::new("not_found", format!("edition {id}")))?;

        let work_title = works::Entity::find_by_id(edition.work_id)
            .one(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?
            .map(|w| w.title);

        let author_rows = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(*id))
            .all(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut author_names = Vec::new();
        for link in author_rows {
            if let Some(a) = authors::Entity::find_by_id(link.author_id)
                .one(db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
            {
                author_names.push(a.name);
            }
        }

        let identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(*id))
            .all(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut isbns = Vec::new();
        for link in identifier_rows {
            if let Some(i) = identifiers::Entity::find_by_id(link.identifier_id)
                .one(db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
                && i.kind.eq_ignore_ascii_case("isbn")
            {
                isbns.push(i.value);
            }
        }

        let publisher_rows = edition_publishers::Entity::find()
            .filter(edition_publishers::Column::EditionId.eq(*id))
            .all(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        let mut publisher_names = Vec::new();
        for link in publisher_rows {
            if let Some(p) = publishers::Entity::find_by_id(link.publisher_id)
                .one(db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
            {
                publisher_names.push(p.name);
            }
        }

        let language = match edition.language_id {
            Some(lid) => languages::Entity::find_by_id(lid)
                .one(db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
                .map(|l| l.name),
            None => None,
        };
        let format = match edition.format_id {
            Some(fid) => formats::Entity::find_by_id(fid)
                .one(db)
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
    let db = state.db.db_conn();
    let guard = state.search_index.read().await;
    add_edition_tags_inner(&db, guard.as_ref(), &edition_ids, &tag).await
}

/// Attach the (trimmed) tag `tag` to every edition, creating the tag on first
/// use. Reindexes `index` when any link was added; `None` skips reindexing.
pub(crate) async fn add_edition_tags_inner(
    db: &DatabaseConnection,
    index: Option<&SearchIndex>,
    edition_ids: &[DbId],
    tag: &str,
) -> Result<TagMutationOutcome, BulkError> {
    use livtet_core::data::entities::{edition_tags, tags};
    use livtet_types::now_primitive;

    let name = tag.trim().to_string();
    if name.is_empty() {
        return Err(BulkError::new("invalid_input", "tag name is empty"));
    }

    let existing = tags::Entity::find()
        .filter(tags::Column::Name.eq(name.clone()))
        .one(db)
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
                .insert(db)
                .await
                .map_err(|e| BulkError::new("database", e.to_string()))?
        }
    };

    let mut changed = 0;
    for id in edition_ids {
        let already = edition_tags::Entity::find()
            .filter(edition_tags::Column::EditionId.eq(*id))
            .filter(edition_tags::Column::TagId.eq(tag.id))
            .one(db)
            .await
            .map_err(|e| BulkError::new("database", e.to_string()))?;
        if already.is_some() {
            continue;
        }
        edition_tags::ActiveModel {
            edition_id: Set(*id),
            tag_id: Set(tag.id),
        }
        .insert(db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;
        changed += 1;
    }

    if changed > 0
        && let Some(index) = index
    {
        index
            .reindex(db)
            .await
            .map_err(|e| BulkError::new("index", e.to_string()))?;
    }

    Ok(TagMutationOutcome {
        tag: crate::commands::search::FilterOption {
            id: tag.id,
            label: tag.name,
            flag_emoji: None,
            logo_url: None,
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
    let db = state.db.db_conn();
    let guard = state.search_index.read().await;
    remove_edition_tags_inner(&db, guard.as_ref(), edition_ids, tag_id).await
}

/// Detach tag `tag_id` from the given editions. Reindexes `index` when any
/// link was removed; `None` skips reindexing.
pub(crate) async fn remove_edition_tags_inner(
    db: &DatabaseConnection,
    index: Option<&SearchIndex>,
    edition_ids: Vec<DbId>,
    tag_id: DbId,
) -> Result<TagMutationOutcome, BulkError> {
    use livtet_core::data::entities::{edition_tags, tags};

    let tag = tags::Entity::find_by_id(tag_id)
        .one(db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?
        .ok_or_else(|| BulkError::new("not_found", "tag"))?;

    let result = edition_tags::Entity::delete_many()
        .filter(edition_tags::Column::EditionId.is_in(edition_ids))
        .filter(edition_tags::Column::TagId.eq(tag_id))
        .exec(db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    let changed = result.rows_affected as i32;
    if changed > 0
        && let Some(index) = index
    {
        index
            .reindex(db)
            .await
            .map_err(|e| BulkError::new("index", e.to_string()))?;
    }

    Ok(TagMutationOutcome {
        tag: crate::commands::search::FilterOption {
            id: tag.id,
            label: tag.name,
            flag_emoji: None,
            logo_url: None,
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
    let guard = state.search_index.read().await;
    let index = guard
        .as_ref()
        .ok_or_else(|| BulkError::new("index", "search index unavailable"))?;
    let db = state.db.db_conn();
    matching_edition_ids_inner(index, &db, query.as_deref().unwrap_or(""), filters).await
}

/// Every edition of every work the index matches for `query` + `filters`.
pub(crate) async fn matching_edition_ids_inner(
    index: &SearchIndex,
    db: &DatabaseConnection,
    query: &str,
    filters: Option<crate::commands::search::EditionFilters>,
) -> Result<Vec<DbId>, BulkError> {
    let (built, _opts) =
        crate::commands::search::build_filtered(index, db, query, filters.unwrap_or_default())
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
        .all(db)
        .await
        .map_err(|e| BulkError::new("database", e.to_string()))?;

    Ok(rows.into_iter().map(|e| e.id).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use camino::Utf8PathBuf;
    use livtet_core::data::entities::{
        authors, edition_authors, edition_identifiers, edition_publishers, edition_tags, formats,
        identifiers, languages, publishers, tags, works,
    };
    use livtet_core::data::orm::PaginatorTrait;
    use livtet_core::data::{Kind, TestDb};
    use livtet_types::{KnownFormats, now_primitive};

    const HEADER: &str =
        "edition_id,work_id,title,authors,isbn,publisher,language,format,published_date";

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
    fn csv_escape_empty_and_cr() {
        assert_eq!(csv_escape(""), "");
        assert_eq!(csv_escape("a\rb"), "\"a\rb\"");
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

    #[test]
    fn pure_csv_row_joins() {
        assert!(super::csv_row(&["a", "b"]).contains(','));
    }

    // ---- fixtures ----------------------------------------------------------

    fn utf8_dir(dir: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 temp dir")
    }

    async fn seed_work(db: &DatabaseConnection, title: &str) -> DbId {
        let id = DbId::new();
        works::ActiveModel {
            id: Set(id),
            title: Set(title.to_string()),
            description: Set(None),
            sort_title: Set(None),
            series_type: Set(None),
            language_id: Set(None),
            preferred_edition_id: Set(None),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("work");
        id
    }

    async fn seed_edition_with(
        db: &DatabaseConnection,
        work_id: DbId,
        title: Option<&str>,
        published_date: Option<time::Date>,
        format_id: Option<DbId>,
        language_id: Option<DbId>,
    ) -> DbId {
        let id = DbId::new();
        editions::ActiveModel {
            id: Set(id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(title.map(str::to_string)),
            published_date: Set(published_date),
            format_id: Set(format_id),
            language_id: Set(language_id),
            notes: Set(None),
            description: Set(None),
            format_metadata: Set(None),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");
        id
    }

    async fn seed_edition(db: &DatabaseConnection, work_id: DbId, title: Option<&str>) -> DbId {
        seed_edition_with(db, work_id, title, None, None, None).await
    }

    async fn seed_inventory(
        db: &DatabaseConnection,
        edition_id: DbId,
        file_path: Option<String>,
        cover_path: Option<String>,
    ) {
        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(file_path),
            cover_path: Set(cover_path),
            blurhash: Set(None),
            dominant_color: Set(None),
            file_hash: Set(None),
            file_size_bytes: Set(None),
            file_format: Set(Some("epub".to_string())),
            notes: Set(None),
            added_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("inventory");
    }

    async fn seed_manual_cover(db: &DatabaseConnection, edition_id: DbId, cover_path: String) {
        edition_specific_covers::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            cover_path: Set(cover_path),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("manual cover");
    }

    async fn seed_tag(db: &DatabaseConnection, name: &str) -> DbId {
        let id = DbId::new();
        tags::ActiveModel {
            id: Set(id),
            name: Set(name.to_string()),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("tag");
        id
    }

    async fn link_tag(db: &DatabaseConnection, edition_id: DbId, tag_id: DbId) {
        edition_tags::ActiveModel {
            edition_id: Set(edition_id),
            tag_id: Set(tag_id),
        }
        .insert(db)
        .await
        .expect("edition tag");
    }

    /// `(edition_id, tag_id)` links as sorted strings, for order-free compares.
    async fn tag_links(db: &DatabaseConnection) -> Vec<(String, String)> {
        let mut links: Vec<(String, String)> = edition_tags::Entity::find()
            .all(db)
            .await
            .expect("edition tags")
            .into_iter()
            .map(|l| (l.edition_id.to_string(), l.tag_id.to_string()))
            .collect();
        links.sort();
        links
    }

    fn sorted(ids: &[DbId]) -> Vec<String> {
        let mut out: Vec<String> = ids.iter().map(ToString::to_string).collect();
        out.sort();
        out
    }

    async fn edition_exists(db: &DatabaseConnection, id: DbId) -> bool {
        editions::Entity::find_by_id(id)
            .one(db)
            .await
            .expect("edition query")
            .is_some()
    }

    fn open_index(dir: &tempfile::TempDir) -> SearchIndex {
        SearchIndex::open(&utf8_dir(dir)).expect("search index")
    }

    async fn indexed_work_ids(index: &SearchIndex) -> Vec<String> {
        let ids = index
            .matching_work_ids_from_query(&livtet_search::AllQuery)
            .await
            .expect("index query");
        sorted(&ids)
    }

    // ---- collect_deletion_plans ---------------------------------------------

    #[tokio::test]
    async fn deletion_plans_gather_library_file_and_every_cover() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let work = seed_work(&db, "Work").await;
        let with_files = seed_edition(&db, work, Some("A")).await;
        let bare = seed_edition(&db, work, Some("B")).await;
        seed_inventory(
            &db,
            with_files,
            Some("/lib/a.epub".into()),
            Some("/covers/a/cover.jpg".into()),
        )
        .await;
        seed_manual_cover(&db, with_files, "/manual/a/1.jpg".into()).await;
        seed_manual_cover(&db, with_files, "/manual/a/2.jpg".into()).await;

        let plans = collect_deletion_plans(&db, &[bare, with_files])
            .await
            .expect("plans");

        // One plan per requested id, in request order.
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].edition_id, bare);
        assert_eq!(plans[0].file_path, None);
        assert!(plans[0].cover_paths.is_empty());

        assert_eq!(plans[1].edition_id, with_files);
        assert_eq!(plans[1].file_path.as_deref(), Some("/lib/a.epub"));
        // The inventory cover leads; manual covers follow in any order.
        assert_eq!(plans[1].cover_paths[0], "/covers/a/cover.jpg");
        let mut manual = plans[1].cover_paths[1..].to_vec();
        manual.sort();
        assert_eq!(manual, vec!["/manual/a/1.jpg", "/manual/a/2.jpg"]);
    }

    // ---- delete_editions_inner ----------------------------------------------

    #[tokio::test]
    async fn delete_removes_rows_files_and_covers_of_only_the_selected_editions() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let root = utf8_dir(&dir);

        let work = seed_work(&db, "Work").await;
        let doomed = seed_edition(&db, work, Some("Doomed")).await;
        let kept = seed_edition(&db, work, Some("Kept")).await;

        let book = root.join("doomed.epub");
        let cover_dir = root.join("covers").join("doomed");
        let cover = cover_dir.join("cover.jpg");
        // The manual cover shares a directory with an unrelated file, which
        // must survive the empty-parent cleanup.
        let shared_dir = root.join("manual");
        let manual = shared_dir.join("doomed.jpg");
        let neighbour = shared_dir.join("neighbour.jpg");
        let kept_book = root.join("kept.epub");
        fs_err::create_dir_all(&cover_dir).unwrap();
        fs_err::create_dir_all(&shared_dir).unwrap();
        for path in [&book, &cover, &manual, &neighbour, &kept_book] {
            fs_err::write(path, b"x").unwrap();
        }

        seed_inventory(&db, doomed, Some(book.to_string()), Some(cover.to_string())).await;
        seed_manual_cover(&db, doomed, manual.to_string()).await;
        seed_inventory(&db, kept, Some(kept_book.to_string()), None).await;

        let outcome = delete_editions_inner(&db, None, &[doomed])
            .await
            .expect("delete");

        assert_eq!(outcome.deleted, 1);
        assert_eq!(outcome.files_removed, 1);
        assert_eq!(outcome.covers_removed, 2);
        assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);

        assert!(!book.exists());
        assert!(!cover.exists());
        assert!(!manual.exists());
        assert!(!cover_dir.exists(), "emptied cover dir is pruned");
        assert!(
            neighbour.exists(),
            "unrelated file in a shared dir survives"
        );
        assert!(shared_dir.exists(), "non-empty cover dir is kept");

        assert!(!edition_exists(&db, doomed).await);
        let inventory_left = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(doomed))
            .count(&db)
            .await
            .unwrap();
        let manual_left = edition_specific_covers::Entity::find()
            .filter(edition_specific_covers::Column::EditionId.eq(doomed))
            .count(&db)
            .await
            .unwrap();
        assert_eq!((inventory_left, manual_left), (0, 0));

        assert!(edition_exists(&db, kept).await);
        assert!(kept_book.exists());
    }

    #[tokio::test]
    async fn delete_treats_already_missing_files_as_success() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let work = seed_work(&db, "Work").await;
        let edition = seed_edition(&db, work, None).await;
        seed_inventory(
            &db,
            edition,
            Some(utf8_dir(&dir).join("gone.epub").to_string()),
            Some(utf8_dir(&dir).join("gone.jpg").to_string()),
        )
        .await;

        let outcome = delete_editions_inner(&db, None, &[edition])
            .await
            .expect("delete");

        assert_eq!(outcome.deleted, 1);
        assert_eq!((outcome.files_removed, outcome.covers_removed), (0, 0));
        assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
        assert!(!edition_exists(&db, edition).await);
    }

    #[tokio::test]
    async fn delete_reports_unremovable_files_without_aborting() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        // Directories cannot be unlinked with remove_file, regardless of
        // privileges, so they stand in for any non-NotFound I/O failure.
        let file_as_dir = utf8_dir(&dir).join("book.epub");
        let cover_as_dir = utf8_dir(&dir).join("cover.jpg");
        fs_err::create_dir_all(&file_as_dir).unwrap();
        fs_err::create_dir_all(&cover_as_dir).unwrap();

        let work = seed_work(&db, "Work").await;
        let edition = seed_edition(&db, work, None).await;
        seed_inventory(
            &db,
            edition,
            Some(file_as_dir.to_string()),
            Some(cover_as_dir.to_string()),
        )
        .await;

        let outcome = delete_editions_inner(&db, None, &[edition])
            .await
            .expect("delete still succeeds");

        assert_eq!(outcome.deleted, 1);
        assert_eq!((outcome.files_removed, outcome.covers_removed), (0, 0));
        assert_eq!(outcome.skipped.len(), 2, "{:?}", outcome.skipped);
        assert!(outcome.skipped.iter().all(|s| s.edition_id == edition));
        assert!(outcome.skipped[0].reason.starts_with("file:"));
        assert!(outcome.skipped[1].reason.starts_with("cover:"));
        assert!(file_as_dir.is_dir() && cover_as_dir.is_dir());
        assert!(
            !edition_exists(&db, edition).await,
            "row deletion is not rolled back"
        );
    }

    #[tokio::test]
    async fn delete_drops_the_editions_from_the_search_index() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let index_dir = tempfile::tempdir().expect("index dir");
        let index = open_index(&index_dir);

        let doomed_work = seed_work(&db, "Doomed").await;
        let kept_work = seed_work(&db, "Kept").await;
        let doomed = seed_edition(&db, doomed_work, Some("Doomed")).await;
        seed_edition(&db, kept_work, Some("Kept")).await;
        index.reindex(&db).await.expect("reindex");
        assert_eq!(
            indexed_work_ids(&index).await,
            sorted(&[doomed_work, kept_work])
        );

        let outcome = delete_editions_inner(&db, Some(&index), &[doomed])
            .await
            .expect("delete");

        assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
        assert_eq!(indexed_work_ids(&index).await, sorted(&[kept_work]));
    }
    #[tokio::test]
    async fn delete_empty_list_is_success_with_zero_counts() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let outcome = delete_editions_inner(&db, None, &[]).await.expect("delete");
        assert_eq!(outcome.deleted, 0);
        assert_eq!(outcome.files_removed, 0);
        assert_eq!(outcome.covers_removed, 0);
        assert!(outcome.skipped.is_empty());
    }

    // ---- export_editions_csv_inner ------------------------------------------

    #[tokio::test]
    async fn export_renders_joined_metadata_and_escapes_fields() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out = utf8_dir(&dir).join("export.csv").to_string();

        let language = languages::Entity::find()
            .one(&db)
            .await
            .unwrap()
            .expect("seeded language");
        let format_id: DbId = KnownFormats::Epub.into();
        let format = formats::Entity::find_by_id(format_id)
            .one(&db)
            .await
            .unwrap()
            .expect("seeded format");

        let work = seed_work(&db, "Work Title").await;
        let date = time::Date::from_calendar_date(1965, time::Month::August, 1).unwrap();
        let edition = seed_edition_with(
            &db,
            work,
            Some("Dune, Part \"One\""),
            Some(date),
            Some(format_id),
            Some(language.id),
        )
        .await;

        for name in ["Frank Herbert", "Brian Herbert"] {
            let author_id = DbId::new();
            authors::ActiveModel {
                id: Set(author_id),
                name: Set(name.to_string()),
            }
            .insert(&db)
            .await
            .unwrap();
            edition_authors::ActiveModel {
                edition_id: Set(edition),
                author_id: Set(author_id),
                role: Set("author".to_string()),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        // Only isbn-kind identifiers (any case) reach the isbn column.
        for (value, kind) in [
            ("urn:isbn:9780441013593", "ISBN"),
            ("urn:wikidata:Q190192", "wikidata"),
        ] {
            let identifier_id = DbId::new();
            identifiers::ActiveModel {
                id: Set(identifier_id),
                value: Set(value.to_string()),
                kind: Set(kind.to_string()),
            }
            .insert(&db)
            .await
            .unwrap();
            edition_identifiers::ActiveModel {
                edition_id: Set(edition),
                identifier_id: Set(identifier_id),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let publisher_id = DbId::new();
        publishers::ActiveModel {
            id: Set(publisher_id),
            name: Set("Chilton".to_string()),
            website: Set(None),
            logo_url: Set(None),
            created_at: Set(now_primitive()),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .unwrap();
        edition_publishers::ActiveModel {
            edition_id: Set(edition),
            publisher_id: Set(publisher_id),
        }
        .insert(&db)
        .await
        .unwrap();

        let outcome = export_editions_csv_inner(&db, &[edition], out.clone())
            .await
            .expect("export");
        assert_eq!(outcome.rows, 1);
        assert_eq!(outcome.path, out);

        let body = fs_err::read_to_string(&out).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert!(body.ends_with('\n'));
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], HEADER);

        let row = |authors: &str| {
            format!(
                "{edition},{work},\"Dune, Part \"\"One\"\"\",{authors},urn:isbn:9780441013593,Chilton,{},{},1965-08-01",
                csv_escape(&language.name),
                csv_escape(&format.name),
            )
        };
        // Author order follows the junction table, which is unspecified.
        let expected = [
            row("Frank Herbert; Brian Herbert"),
            row("Brian Herbert; Frank Herbert"),
        ];
        assert!(expected.iter().any(|e| e == lines[1]), "row: {}", lines[1]);
    }

    #[tokio::test]
    async fn export_falls_back_to_work_title_and_blanks_missing_columns() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out = utf8_dir(&dir).join("export.csv").to_string();
        let work = seed_work(&db, "The Work").await;
        let edition = seed_edition(&db, work, None).await;

        export_editions_csv_inner(&db, &[edition], out.clone())
            .await
            .expect("export");

        let body = fs_err::read_to_string(&out).unwrap();
        assert_eq!(body, format!("{HEADER}\n{edition},{work},The Work,,,,,,\n"));
    }

    #[tokio::test]
    async fn export_replaces_existing_file_in_request_order_without_temp_leftovers() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out_path = utf8_dir(&dir).join("export.csv");
        let out = out_path.to_string();
        fs_err::write(&out_path, b"stale contents\nthat must go\n").unwrap();

        let work = seed_work(&db, "Work").await;
        let first = seed_edition(&db, work, Some("First")).await;
        let second = seed_edition(&db, work, Some("Second")).await;

        let outcome = export_editions_csv_inner(&db, &[second, first], out.clone())
            .await
            .expect("export");

        assert_eq!(outcome.rows, 2);
        assert!(!utf8_dir(&dir).join("export.csv.tmp").exists());
        let body = fs_err::read_to_string(&out).unwrap();
        assert_eq!(
            body,
            format!("{HEADER}\n{second},{work},Second,,,,,,\n{first},{work},First,,,,,,\n")
        );
    }

    #[tokio::test]
    async fn export_of_unknown_edition_fails_without_writing() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out_path = utf8_dir(&dir).join("export.csv");
        let work = seed_work(&db, "Work").await;
        let known = seed_edition(&db, work, Some("Known")).await;

        let err = export_editions_csv_inner(&db, &[known, DbId::new()], out_path.to_string())
            .await
            .expect_err("unknown edition");

        assert_eq!(err.code, "not_found");
        assert!(!out_path.exists());
        assert!(!utf8_dir(&dir).join("export.csv.tmp").exists());
    }

    #[tokio::test]
    async fn export_into_missing_directory_is_an_io_error() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out = utf8_dir(&dir)
            .join("no-such-dir")
            .join("export.csv")
            .to_string();

        let err = export_editions_csv_inner(&db, &[], out)
            .await
            .expect_err("unwritable path");

        assert_eq!(err.code, "io");
    }
    #[tokio::test]
    async fn export_empty_list_writes_header_only() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let dir = tempfile::tempdir().expect("temp dir");
        let out = utf8_dir(&dir).join("empty.csv").to_string();

        let outcome = export_editions_csv_inner(&db, &[], out.clone())
            .await
            .expect("export");
        assert_eq!(outcome.rows, 0);
        assert_eq!(outcome.path, out);
        assert_eq!(fs_err::read_to_string(&out).unwrap(), format!("{HEADER}\n"));
    }

    // ---- tags ----------------------------------------------------------------

    #[tokio::test]
    async fn adding_a_tag_trims_reuses_the_row_and_counts_only_new_links() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let work = seed_work(&db, "Work").await;
        let a = seed_edition(&db, work, None).await;
        let b = seed_edition(&db, work, None).await;

        let first = add_edition_tags_inner(&db, None, &[a], "  sci-fi  ")
            .await
            .expect("first add");
        assert_eq!(first.tag.label, "sci-fi");
        assert_eq!(first.changed, 1);

        let second = add_edition_tags_inner(&db, None, &[a, b], "sci-fi")
            .await
            .expect("second add");
        assert_eq!(second.tag.id, first.tag.id, "existing tag is reused");
        assert_eq!(second.changed, 1, "already-tagged edition is not recounted");

        let named = tags::Entity::find()
            .filter(tags::Column::Name.eq("sci-fi"))
            .count(&db)
            .await
            .unwrap();
        assert_eq!(named, 1);
        let tag = first.tag.id.to_string();
        let mut expected = vec![(a.to_string(), tag.clone()), (b.to_string(), tag)];
        expected.sort();
        assert_eq!(tag_links(&db).await, expected);
    }

    #[tokio::test]
    async fn adding_a_blank_tag_is_rejected_without_side_effects() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let work = seed_work(&db, "Work").await;
        let edition = seed_edition(&db, work, None).await;

        let err = add_edition_tags_inner(&db, None, &[edition], " \t ")
            .await
            .expect_err("blank tag");

        assert_eq!(err.code, "invalid_input");
        assert_eq!(tags::Entity::find().count(&db).await.unwrap(), 0);
        assert!(tag_links(&db).await.is_empty());
    }

    #[tokio::test]
    async fn removing_a_tag_detaches_only_that_tag_from_the_given_editions() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let work = seed_work(&db, "Work").await;
        let a = seed_edition(&db, work, None).await;
        let b = seed_edition(&db, work, None).await;
        let c = seed_edition(&db, work, None).await;
        let target = seed_tag(&db, "target").await;
        let other = seed_tag(&db, "other").await;
        for edition in [a, b, c] {
            link_tag(&db, edition, target).await;
            link_tag(&db, edition, other).await;
        }

        let outcome = remove_edition_tags_inner(&db, None, vec![a, b], target)
            .await
            .expect("remove");
        assert_eq!(outcome.changed, 2);
        assert_eq!(outcome.tag.id, target);
        assert_eq!(outcome.tag.label, "target");

        let mut expected = vec![
            (c.to_string(), target.to_string()),
            (a.to_string(), other.to_string()),
            (b.to_string(), other.to_string()),
            (c.to_string(), other.to_string()),
        ];
        expected.sort();
        assert_eq!(tag_links(&db).await, expected);
        assert!(
            tags::Entity::find_by_id(target)
                .one(&db)
                .await
                .unwrap()
                .is_some(),
            "the tag itself survives"
        );

        let again = remove_edition_tags_inner(&db, None, vec![a, b], target)
            .await
            .expect("repeat remove");
        assert_eq!(again.changed, 0);
    }

    #[tokio::test]
    async fn removing_an_unknown_tag_is_not_found() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let work = seed_work(&db, "Work").await;
        let edition = seed_edition(&db, work, None).await;

        let err = remove_edition_tags_inner(&db, None, vec![edition], DbId::new())
            .await
            .expect_err("unknown tag");

        assert_eq!(err.code, "not_found");
    }
    #[tokio::test]
    async fn removing_tag_with_index_reindexes_when_links_changed() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let index_dir = tempfile::tempdir().expect("index dir");
        let index = open_index(&index_dir);
        let work = seed_work(&db, "Work").await;
        let ed = seed_edition(&db, work, Some("Ed")).await;
        let tag = seed_tag(&db, "tag").await;
        link_tag(&db, ed, tag).await;
        index.reindex(&db).await.expect("reindex");

        let outcome = remove_edition_tags_inner(&db, Some(&index), vec![ed], tag)
            .await
            .expect("remove with index");
        assert_eq!(outcome.changed, 1);
    }

    // ---- matching_edition_ids_inner ----------------------------------------

    #[tokio::test]
    async fn tag_changes_reindex_so_tag_filters_track_them() {
        use crate::commands::search::EditionFilters;

        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let index_dir = tempfile::tempdir().expect("index dir");
        let index = open_index(&index_dir);

        let dune = seed_work(&db, "Dune").await;
        let emma = seed_work(&db, "Emma").await;
        let dune_ed = seed_edition(&db, dune, Some("Dune")).await;
        seed_edition(&db, emma, Some("Emma")).await;
        index.reindex(&db).await.expect("reindex");

        let added = add_edition_tags_inner(&db, Some(&index), &[dune_ed], "favourite")
            .await
            .expect("add");
        let by_tag = || EditionFilters {
            tag_ids: vec![added.tag.id],
            ..Default::default()
        };
        let tagged = matching_edition_ids_inner(&index, &db, "", Some(by_tag()))
            .await
            .expect("match tagged");
        assert_eq!(sorted(&tagged), sorted(&[dune_ed]));

        remove_edition_tags_inner(&db, Some(&index), vec![dune_ed], added.tag.id)
            .await
            .expect("remove");
        let untagged = matching_edition_ids_inner(&index, &db, "", Some(by_tag()))
            .await
            .expect("match untagged");
        assert!(untagged.is_empty(), "{untagged:?}");
    }

    #[tokio::test]
    async fn free_text_matching_returns_editions_of_matching_works() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let index_dir = tempfile::tempdir().expect("index dir");
        let index = open_index(&index_dir);

        let dune = seed_work(&db, "Dune").await;
        let emma = seed_work(&db, "Emma").await;
        let dune_ed = seed_edition(&db, dune, Some("Dune")).await;
        let emma_ed = seed_edition(&db, emma, Some("Emma")).await;
        index.reindex(&db).await.expect("reindex");

        let hits = matching_edition_ids_inner(&index, &db, "Dune", None)
            .await
            .expect("free text");
        assert_eq!(sorted(&hits), sorted(&[dune_ed]));

        let everything = matching_edition_ids_inner(&index, &db, "", None)
            .await
            .expect("match all");
        assert_eq!(sorted(&everything), sorted(&[dune_ed, emma_ed]));

        let nothing = matching_edition_ids_inner(&index, &db, "xylophonic", None)
            .await
            .expect("no match");
        assert!(nothing.is_empty(), "{nothing:?}");
    }
    #[tokio::test]
    async fn matching_empty_work_ids_returns_empty() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let index_dir = tempfile::tempdir().expect("index dir");
        let index = open_index(&index_dir);
        // Empty index has no works, so build_query yields nothing.
        let empty = matching_edition_ids_inner(&index, &db, "anything", None)
            .await
            .expect("empty work ids returns ok");
        assert!(empty.is_empty(), "{empty:?}");
    }
}
