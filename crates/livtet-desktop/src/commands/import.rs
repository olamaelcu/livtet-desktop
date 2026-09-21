//! `import_file` — import a book through the importer selected for its extension.
//!
//! Fail-closed: the selected importer must return a title, a creator, and at
//! least one valid ISBN, or nothing is written. All catalog rows are created
//! in a single transaction; re-importing the identical file is a no-op that
//! returns the pre-existing edition id.

use serde::Serialize;
use sha2::Digest;
use specta::Type;
use tauri::State;

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
    edition_subjects, editions, identifiers, languages, publishers, subjects, works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use livtet_importer::{EpubImporter, Importer, ImporterMeta};
use livtet_types::{CommonLanguages, DbId, Isbn, KnownFormats, Urn, now_primitive};

use super::importers::remote_importer_metadata;
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
    /// Stable machine code: `parse` | `database` | `index` | `io` | `importer` | `unsupported`.
    pub(crate) code: String,
    pub(crate) message: String,
}

impl ImportError {
    pub(crate) fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl From<livtet_core::data::orm::DbErr> for ImportError {
    fn from(err: livtet_core::data::orm::DbErr) -> Self {
        Self::new("database", err)
    }
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
    language: Option<&str>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileImporterSource {
    NativeEpub,
    Remote,
}

fn validate_importer_meta(meta: &ImporterMeta) -> Result<Vec<String>, ImportError> {
    if meta.title.trim().is_empty() {
        return Err(ImportError::new(
            "parse",
            "importer metadata is missing a title",
        ));
    }
    if meta.contributors.is_empty()
        || meta
            .contributors
            .iter()
            .any(|contributor| contributor.name.trim().is_empty())
    {
        return Err(ImportError::new(
            "parse",
            "importer metadata is missing a creator",
        ));
    }
    if meta.isbns.is_empty() {
        return Err(ImportError::new(
            "parse",
            "importer metadata is missing an ISBN",
        ));
    }
    let mut canonical_isbns = Vec::with_capacity(meta.isbns.len());
    for isbn in &meta.isbns {
        let canonical = Isbn::parse(isbn).map_err(|error| ImportError::new("parse", error))?;
        canonical_isbns.push(canonical.as_str().to_string());
    }
    Ok(canonical_isbns)
}

/// Normalizes a contributor role for storage, reporting whether it is one of
/// the core MARC relator codes the native importer produces.
fn normalize_contributor_role(role: Option<&str>) -> (String, bool) {
    let normalized = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("aut")
        .to_string();
    let known = matches!(normalized.as_str(), "aut" | "edt" | "trl" | "ill");
    (normalized, known)
}

fn format_id_for_extension(extension: &str) -> Option<DbId> {
    if extension.eq_ignore_ascii_case("epub") {
        Some(KnownFormats::Epub.into())
    } else if extension.eq_ignore_ascii_case("pdf") {
        Some(KnownFormats::Pdf.into())
    } else {
        None
    }
}

fn importer_source_for_extension(extension: &str) -> FileImporterSource {
    if extension.eq_ignore_ascii_case("epub") {
        FileImporterSource::NativeEpub
    } else {
        FileImporterSource::Remote
    }
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip_all, fields(path))]
pub async fn import_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    let file_path = std::path::PathBuf::from(&path);
    let extension = file_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (meta, source_bytes) = match importer_source_for_extension(&extension) {
        FileImporterSource::NativeEpub => (
            EpubImporter
                .read_metadata(path.clone())
                .map_err(|error| ImportError::new("parse", error))?,
            None,
        ),
        FileImporterSource::Remote => {
            let read = remote_importer_metadata(
                state.plugin_host_path.clone().into_std_path_buf(),
                state.plugin_host_config.clone().into_std_path_buf(),
                state.plugins_dir.clone().into_std_path_buf(),
                file_path.clone(),
            )
            .await?;
            (read.meta, Some(read.source_bytes))
        }
    };
    let isbn_strings = validate_importer_meta(&meta)?;

    // Content hash for dedup. Remote imports reuse the exact bytes served
    // through `fs_read`; native parsing still opens the file separately.
    let bytes = match source_bytes {
        Some(bytes) => bytes,
        None => fs_err::tokio::read(&file_path)
            .await
            .map_err(|e| ImportError::new("io", format!("reading file: {e}")))?,
    };
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
            isbns: isbn_strings.clone(),
            duplicate: true,
        });
    }

    let title = meta.title.clone();
    let txn = db.begin().await?;

    let work_id = DbId::new();
    let language_id = resolve_language(&txn, meta.language.as_deref()).await?;

    let now = now_primitive();
    works::ActiveModel {
        id: Set(work_id),
        title: Set(title.clone()),
        description: Set(meta.description.clone()),
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
    let cover = meta
        .cover
        .as_ref()
        .map(|cover| {
            let data = STANDARD.decode(&cover.data_base64).map_err(|error| {
                ImportError::new("parse", format!("invalid cover data: {error}"))
            })?;
            Ok::<_, ImportError>((cover.mime.clone(), data))
        })
        .transpose()?;
    // ADR-0008 path convention: {app_local_data_dir}/covers/{inventory_id}/cover.{ext}.
    // covers_dir already resolves to {app_local_data_dir}/covers (Tauri identifier == BUNDLE_ID).
    let cover_path = cover.as_ref().map(|(mime, _)| {
        state
            .covers_dir
            .join(inventory_id.to_string())
            .join(format!("cover.{}", cover_extension(mime)))
            .to_string()
    });

    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some(title.clone())),
        // Preserve legacy import behavior: unrepresentable remote dates become NULL
        // rather than failing the whole import.
        published_date: Set(meta.published.as_ref().and_then(|p| {
            time::Date::from_calendar_date(
                p.year,
                time::Month::try_from(p.month.unwrap_or(1)).unwrap_or(time::Month::January),
                p.day.unwrap_or(1),
            )
            .ok()
        })),
        format_id: Set(format_id_for_extension(&extension)),
        language_id: Set(language_id),
        notes: Set(None),
        description: Set(meta.description.clone()),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    for contributor in &meta.contributors {
        let author_id = upsert_author(&txn, &contributor.name).await?;
        let (role, known) = normalize_contributor_role(contributor.role.as_deref());
        if !known {
            tracing::debug!(role, "storing non-core contributor role");
        }
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set(role),
        }
        .insert(&txn)
        .await?;
    }

    if let Some(publisher) = &meta.publisher {
        let publisher_id = upsert_publisher(&txn, publisher).await?;
        edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
        }
        .insert(&txn)
        .await?;
    }

    for subject in &meta.subjects {
        let subject_id = upsert_subject(&txn, subject).await?;
        edition_subjects::ActiveModel {
            edition_id: Set(edition_id),
            subject_id: Set(subject_id),
        }
        .insert(&txn)
        .await?;
    }

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
        file_format: Set((!extension.is_empty()).then(|| extension.clone())),
        notes: Set(None),
        added_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    txn.commit().await?;

    // Cover bytes go to disk post-commit; a failed write nulls the path
    // (DB stays authoritative, file is re-derivable).
    if let (Some((_, data)), Some(path)) = (&cover, &cover_path) {
        let result = async {
            if let Some(parent) = std::path::Path::new(path.as_str()).parent() {
                fs_err::tokio::create_dir_all(parent)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            fs_err::tokio::write(path, data)
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

#[cfg(test)]
mod tests {
    use livtet_importer::ImporterMeta;

    use super::{
        FileImporterSource, ImportError, format_id_for_extension, importer_source_for_extension,
        normalize_contributor_role, validate_importer_meta,
    };

    #[test]
    fn epub_extension_uses_the_native_importer() {
        assert!(matches!(
            importer_source_for_extension("EPUB"),
            FileImporterSource::NativeEpub
        ));
    }

    #[test]
    fn unknown_extensions_use_remote_importers() {
        assert!(matches!(
            importer_source_for_extension("pdf"),
            FileImporterSource::Remote
        ));
    }

    #[test]
    fn contributor_roles_default_and_flag_non_core_values() {
        assert_eq!(
            normalize_contributor_role(Some("edt")),
            ("edt".to_string(), true)
        );
        assert_eq!(normalize_contributor_role(None), ("aut".to_string(), true));
        assert_eq!(
            normalize_contributor_role(Some("   ")),
            ("aut".to_string(), true)
        );
        assert_eq!(
            normalize_contributor_role(Some("ctb")),
            ("ctb".to_string(), false)
        );
    }

    #[test]
    fn extensions_map_to_known_catalog_formats() {
        assert_eq!(
            format_id_for_extension("epub"),
            Some(livtet_types::KnownFormats::Epub.into())
        );
        assert_eq!(
            format_id_for_extension("PDF"),
            Some(livtet_types::KnownFormats::Pdf.into())
        );
        assert_eq!(format_id_for_extension("txt"), None);
    }

    fn valid_record() -> ImporterMeta {
        ImporterMeta {
            title: "Remote Title".to_string(),
            contributors: vec![livtet_importer::ImporterContributor {
                name: "Remote Author".to_string(),
                role: Some("aut".to_string()),
                file_as: None,
            }],
            isbns: vec!["9781784780609".to_string()],
            other_identifiers: Vec::new(),
            publisher: None,
            language: None,
            published: None,
            description: None,
            subjects: Vec::new(),
            cover: None,
        }
    }

    #[test]
    fn importer_metadata_must_carry_catalog_minimum() {
        assert!(validate_importer_meta(&valid_record()).is_ok());

        let mut record = valid_record();
        record.title = "   ".to_string();
        assert!(matches!(
            validate_importer_meta(&record),
            Err(ImportError { code, .. }) if code == "parse"
        ));

        let mut record = valid_record();
        record.contributors.clear();
        assert!(validate_importer_meta(&record).is_err());

        let mut record = valid_record();
        record.contributors[0].name = "   ".to_string();
        assert!(validate_importer_meta(&record).is_err());

        let mut record = valid_record();
        record.isbns.clear();
        assert!(validate_importer_meta(&record).is_err());

        let mut record = valid_record();
        record.isbns = vec!["not-an-isbn".to_string()];
        assert!(matches!(
            validate_importer_meta(&record),
            Err(ImportError { code, .. }) if code == "parse"
        ));
    }

    #[test]
    fn importer_isbns_are_canonicalized_before_persistence() {
        let mut record = valid_record();
        record.isbns = vec!["978-1-78478-060-9".to_string(), "0-306-40615-2".to_string()];

        assert_eq!(
            validate_importer_meta(&record).expect("ISBN variants must canonicalize"),
            vec!["9781784780609".to_string(), "9780306406157".to_string()]
        );
    }
}
