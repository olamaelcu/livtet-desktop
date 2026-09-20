//! `import_epub` — import a book from an EPUB file on disk.
//!
//! Fail-closed: the file must parse, carry a title, a creator, and at least
//! one valid ISBN, or nothing is written. All catalog rows are created in a
//! single transaction; re-importing the identical file is a no-op that
//! returns the pre-existing edition id.

use serde::Serialize;
use sha2::Digest;
use specta::Type;
use tauri::State;

use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
    edition_subjects, editions, identifiers, languages, publishers, subjects, works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use livtet_epub::Role as EpubRole;
use livtet_types::{CommonLanguages, DbId, KnownFormats, Urn, now_primitive};

use crate::types::AppState;

#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportOutcome {
    pub work_id: String,
    pub edition_id: String,
    pub title: String,
    /// Validated ISBN-13 values attached to the edition.
    pub isbns: Vec<String>,
    /// True when the file was already present (hash match) and nothing
    /// was written.
    pub duplicate: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportError {
    /// Stable machine code: `parse` | `database` | `index` | `io`.
    pub code: String,
    pub message: String,
}

impl ImportError {
    fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl From<livtet_epub::EpubError> for ImportError {
    fn from(err: livtet_epub::EpubError) -> Self {
        Self::new("parse", err)
    }
}

impl From<livtet_core::data::orm::DbErr> for ImportError {
    fn from(err: livtet_core::data::orm::DbErr) -> Self {
        Self::new("database", err)
    }
}

/// Map livtet-epub's MARC-flavored role to the `edition_authors.role` string.
fn role_string(role: &EpubRole) -> String {
    match role {
        EpubRole::Author => "aut",
        EpubRole::Editor => "edt",
        EpubRole::Translator => "trl",
        EpubRole::Illustrator => "ill",
        EpubRole::Other(raw) => raw,
    }
    .to_string()
}

/// Find-or-create a name-keyed entity, returning its id. `insert` builds the
/// model with an id; we keep that id in both code paths.
async fn upsert_author(txn: &DatabaseTransaction, name: &str) -> Result<DbId, ImportError> {
    if let Some(found) = authors::Entity::find()
        .filter(authors::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    authors::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

async fn upsert_identifier(
    txn: &DatabaseTransaction,
    value: &str,
    kind: &str,
) -> Result<DbId, ImportError> {
    if let Some(found) = identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(value))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    identifiers::ActiveModel {
        id: Set(id),
        value: Set(value.to_string()),
        kind: Set(kind.to_string()),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

async fn upsert_publisher(txn: &DatabaseTransaction, name: &str) -> Result<DbId, ImportError> {
    if let Some(found) = publishers::Entity::find()
        .filter(publishers::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    publishers::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        website: Set(None),
        logo_url: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

async fn upsert_subject(txn: &DatabaseTransaction, name: &str) -> Result<DbId, ImportError> {
    if let Some(found) = subjects::Entity::find()
        .filter(subjects::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    subjects::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

/// Resolve the `languages` row for the normalized code, creating it when the
/// language is not in the seeded set.
async fn resolve_language(
    txn: &DatabaseTransaction,
    language: Option<&String>,
) -> Result<Option<DbId>, ImportError> {
    let Some(raw) = language else { return Ok(None) };
    let Some(info) = CommonLanguages::normalize_language_code(raw) else {
        return Ok(None);
    };
    if let Some(found) = languages::Entity::find()
        .filter(languages::Column::Code.eq(&info.code))
        .one(txn)
        .await?
    {
        return Ok(Some(found.id));
    }
    let id = DbId::new();
    languages::ActiveModel {
        id: Set(id),
        name: Set(info.english_name),
        code: Set(info.code),
        flag_emoji: Set(info.flag_emoji),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(Some(id))
}

/// File extension for a cover MIME type.
fn cover_extension(mime: &str) -> &str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "img",
    }
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip_all, fields(path))]
pub async fn import_epub(
    path: String,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    let file_path = std::path::PathBuf::from(&path);
    let meta = livtet_epub::read_metadata(&file_path)?;

    // Content hash for dedup.
    let bytes = std::fs::read(&file_path)
        .map_err(|e| ImportError::new("io", format!("reading file: {e}")))?;
    let file_hash = hex::encode(sha2::Sha256::digest(&bytes));

    let db = state.db.db_conn();

    // Dedup: identical bytes already imported?
    if let Some(existing) = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::FileHash.eq(&file_hash))
        .one(&db)
        .await?
    {
        let edition = editions::Entity::find_by_id(existing.edition_id)
            .one(&db)
            .await?;
        let (work_id, title) = edition
            .map(|e| (e.work_id.to_string(), e.title.unwrap_or_default()))
            .unwrap_or_default();
        return Ok(ImportOutcome {
            work_id,
            edition_id: existing.edition_id.to_string(),
            title,
            isbns: meta.isbns.iter().map(|i| i.as_str().to_string()).collect(),
            duplicate: true,
        });
    }

    let title = meta.title.0.clone();
    let txn = db.begin().await?;

    let work_id = DbId::new();
    let language_id = resolve_language(&txn, meta.language.as_ref().map(|l| &l.0)).await?;

    let now = now_primitive();
    works::ActiveModel {
        id: Set(work_id),
        title: Set(title.clone()),
        description: Set(meta.description.as_ref().map(|d| d.0.clone())),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(language_id),
        preferred_edition_id: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    let edition_id = DbId::new();
    let inventory_id = DbId::new();
    // ADR-0008 path convention: {app_local_data_dir}/covers/{inventory_id}/cover.{ext}.
    // covers_dir already resolves to {app_local_data_dir}/covers (Tauri identifier == BUNDLE_ID).
    let cover_path = meta.cover.as_ref().map(|c| {
        state
            .covers_dir
            .join(inventory_id.to_string())
            .join(format!("cover.{}", cover_extension(&c.mime)))
            .to_string()
    });

    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some(title.clone())),
        published_date: Set(meta.published.as_ref().and_then(|p| {
            time::Date::from_calendar_date(
                p.year,
                time::Month::try_from(p.month.unwrap_or(1)).unwrap_or(time::Month::January),
                p.day.unwrap_or(1),
            )
            .ok()
        })),
        format_id: Set(Some(KnownFormats::Epub.into())),
        language_id: Set(language_id),
        notes: Set(None),
        description: Set(meta.description.as_ref().map(|d| d.0.clone())),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    for contributor in &meta.creators {
        let author_id = upsert_author(&txn, &contributor.name).await?;
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set(role_string(&contributor.role)),
        }
        .insert(&txn)
        .await?;
    }

    if let Some(publisher) = &meta.publisher {
        let publisher_id = upsert_publisher(&txn, &publisher.0).await?;
        edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
        }
        .insert(&txn)
        .await?;
    }

    for subject in &meta.subjects {
        let subject_id = upsert_subject(&txn, &subject.0).await?;
        edition_subjects::ActiveModel {
            edition_id: Set(edition_id),
            subject_id: Set(subject_id),
        }
        .insert(&txn)
        .await?;
    }

    let isbn_strings: Vec<String> = meta.isbns.iter().map(|i| i.as_str().to_string()).collect();
    for isbn in &isbn_strings {
        let urn_value = Urn::new("isbn", isbn).to_string();
        let identifier_id = upsert_identifier(&txn, &urn_value, "isbn").await?;
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(&txn)
        .await?;
    }

    let file_size_bytes: i64 = bytes.len() as i64;
    digital_inventory::ActiveModel {
        id: Set(inventory_id),
        edition_id: Set(edition_id),
        file_path: Set(Some(file_path.to_string_lossy().to_string())),
        cover_path: Set(cover_path.clone()),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(Some(file_hash)),
        file_size_bytes: Set(Some(file_size_bytes)),
        file_format: Set(Some("epub".to_string())),
        notes: Set(None),
        added_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    txn.commit().await?;

    // Cover bytes go to disk post-commit; a failed write nulls the path
    // (DB stays authoritative, file is re-derivable).
    if let (Some(cover), Some(path)) = (&meta.cover, &cover_path) {
        let result = async {
            if let Some(parent) = std::path::Path::new(path.as_str()).parent() {
                fs_err::tokio::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            fs_err::tokio::write(path, &cover.data)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        if let Err(err) = result {
            tracing::warn!(%err, %path, "cover write failed; clearing cover_path");
            let mut model: digital_inventory::ActiveModel = digital_inventory::Entity::find()
                .filter(digital_inventory::Column::EditionId.eq(edition_id))
                .one(&db)
                .await?
                .ok_or_else(|| ImportError::new("database", "inventory row vanished post-commit"))?
                .into();
            model.cover_path = Set(None);
            model.update(&db).await?;
        }
    }

    // Surface the new edition in search; add_edition currently rebuilds the
    // whole index (see livtet-search write path comment).
    {
        let guard = state.search_index.read().await;
        if let Some(index) = guard.as_ref() {
            index
                .add_edition(&db, edition_id)
                .await
                .map_err(|e| ImportError::new("index", e))?;
        }
    }

    Ok(ImportOutcome {
        work_id: work_id.to_string(),
        edition_id: edition_id.to_string(),
        title,
        isbns: isbn_strings,
        duplicate: false,
    })
}
