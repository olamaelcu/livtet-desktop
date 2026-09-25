//! `import_file` / `import_files` — import books through the importer selected
//! for their extension. The batch command emits `import://batch` and
//! `import://file` progress events and returns an aggregate per-file result.
//!
//! Fail-closed: the selected importer must return a title and a creator, or
//! nothing is written. ISBNs are optional: any entry that fails to parse is
//! demoted to a non-ISBN identifier, and non-ISBN identifiers are persisted so
//! an edition without an ISBN still has identity. All catalog rows are created
//! in a single transaction; re-importing the identical file is a no-op that
//! returns the pre-existing edition id.

use serde::Serialize;
use sha2::Digest;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State};

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
    edition_subjects, editions, identifiers, languages, publishers, subjects, works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set,
    TransactionTrait,
};
use livtet_importer::{EpubImporter, Importer, ImporterContributor, ImporterMeta};
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

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Import error {}: {}", self.code, self.message))
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportFileResult {
    pub path: String,
    /// Set when the file imported or was already present.
    pub outcome: Option<ImportOutcome>,
    /// Set when the file failed. Exactly one of `outcome` / `error` is set.
    pub error: Option<ImportError>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportBatchResult {
    pub files: Vec<ImportFileResult>,
    pub imported: i32,
    pub duplicated: i32,
    pub failed: i32,
}

const IMPORT_BATCH_EVENT: &str = "import://batch";
const IMPORT_FILE_EVENT: &str = "import://file";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPhase {
    Started,
    Finished,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportBatchProgress {
    pub phase: ImportPhase,
    pub total: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<ImportBatchResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportFileProgress {
    /// Zero-based position of this file within the batch.
    pub index: i32,
    pub total: i32,
    pub path: String,
    pub phase: ImportPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ImportOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ImportError>,
}

fn emit_batch(app: &AppHandle, payload: ImportBatchProgress) {
    if let Err(error) = app.emit(IMPORT_BATCH_EVENT, payload) {
        tracing::warn!(error = %error, "failed to emit import batch event");
    }
}

fn emit_file(app: &AppHandle, payload: ImportFileProgress) {
    if let Err(error) = app.emit(IMPORT_FILE_EVENT, payload) {
        tracing::warn!(error = %error, "failed to emit import file event");
    }
}

/// Pair a file path with its import result. Exactly one side is populated.
fn file_result(path: String, result: Result<ImportOutcome, ImportError>) -> ImportFileResult {
    match result {
        Ok(outcome) => ImportFileResult {
            path,
            outcome: Some(outcome),
            error: None,
        },
        Err(error) => ImportFileResult {
            path,
            outcome: None,
            error: Some(error),
        },
    }
}

/// Fold per-file results into the aggregate the frontend renders.
fn batch_result(files: Vec<ImportFileResult>) -> ImportBatchResult {
    let mut imported = 0;
    let mut duplicated = 0;
    let mut failed = 0;
    for file in &files {
        match (&file.outcome, &file.error) {
            (Some(outcome), _) if outcome.duplicate => duplicated += 1,
            (Some(_), _) => imported += 1,
            (_, Some(_)) => failed += 1,
            (None, None) => {}
        }
    }
    ImportBatchResult {
        files,
        imported,
        duplicated,
        failed,
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

/// Validate the catalog minimum and split identifiers into canonical ISBNs and
/// non-ISBN identifiers.
///
/// A title and at least one non-empty contributor are required; ISBNs are not.
/// Every `isbns` entry is canonicalized with [`Isbn::parse`], and an entry that
/// fails to parse is demoted to `other_identifiers` rather than failing the
/// import. Both returned lists are de-duplicated in first-seen order.
fn validate_importer_meta(meta: &ImporterMeta) -> Result<(Vec<String>, Vec<String>), ImportError> {
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
    let mut canonical_isbns = Vec::with_capacity(meta.isbns.len());
    let mut demoted = Vec::new();
    for isbn in &meta.isbns {
        match Isbn::parse(isbn) {
            Ok(canonical) => canonical_isbns.push(canonical.as_str().to_string()),
            Err(_) => demoted.push(isbn.clone()),
        }
    }
    let mut other_identifiers = meta.other_identifiers.clone();
    other_identifiers.extend(demoted);
    Ok((
        unique_preserving_order(&canonical_isbns),
        unique_preserving_order(&other_identifiers),
    ))
}

/// Derive the stored `identifiers.kind` for a non-ISBN identifier value.
///
/// UUIDs (bare or `urn:uuid:`) are labeled `uuid`; 10-character `[A-Z0-9]`
/// values are labeled `asin`; everything else is `other`.
fn identifier_kind(value: &str) -> &'static str {
    let trimmed = value.trim();
    let normalized = trimmed.to_ascii_lowercase();
    if normalized.starts_with("urn:uuid:") || is_uuid_shape(&normalized) {
        return "uuid";
    }
    if trimmed.chars().count() == 10
        && trimmed
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
    {
        return "asin";
    }
    "other"
}

/// Whether `value` is a bare 36-character hyphenated UUID (`8-4-4-4-12` hex).
/// `value` is expected to be already trimmed and lowercased.
fn is_uuid_shape(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    bytes.iter().enumerate().all(|(index, byte)| {
        if matches!(index, 8 | 13 | 18 | 23) {
            *byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

/// Build the `(stored value, kind)` rows for an edition's identifiers, ISBNs
/// first. Values are de-duplicated on the final stored string so a value can
/// never produce two `edition_identifiers` rows.
fn edition_identifier_rows(
    isbns: &[String],
    other_identifiers: &[String],
) -> Vec<(String, &'static str)> {
    let mut rows: Vec<(String, &'static str)> =
        Vec::with_capacity(isbns.len() + other_identifiers.len());
    for isbn in isbns {
        push_unique_row(&mut rows, Urn::new("isbn", isbn).to_string(), "isbn");
    }
    for identifier in other_identifiers {
        push_unique_row(&mut rows, identifier.clone(), identifier_kind(identifier));
    }
    rows
}

/// Append `(value, kind)` to `rows` unless `value` is already present.
fn push_unique_row(rows: &mut Vec<(String, &'static str)>, value: String, kind: &'static str) {
    if !rows.iter().any(|(seen, _)| seen == &value) {
        rows.push((value, kind));
    }
}

/// Whether `role` is one of the core MARC relator codes the native importer
/// produces.
fn is_core_contributor_role(role: &str) -> bool {
    matches!(role, "aut" | "edt" | "trl" | "ill")
}

/// Normalizes a contributor role for storage, reporting whether it is one of
/// the core MARC relator codes the native importer produces.
fn normalize_contributor_role(role: Option<&str>) -> (String, bool) {
    let normalized = role
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("aut")
        .to_ascii_lowercase();
    let known = is_core_contributor_role(&normalized);
    (normalized, known)
}

/// Collapse a list of strings to its unique non-empty entries, in first-seen
/// order. Importers already dedupe their output, but this fence guarantees a
/// repeated subject or ISBN can never violate a composite primary key on
/// `edition_subjects` / `edition_identifiers` regardless of what it hands us.
fn unique_preserving_order(values: &[String]) -> Vec<String> {
    let mut unique: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !unique.iter().any(|seen| seen == value) {
            unique.push(value.to_string());
        }
    }
    unique
}

/// Collapse contributor records to the `(name, normalized_role)` pairs that
/// become `edition_authors` rows, in input order and deduplicated. The importer
/// already dedupes its output, but this fence guarantees a repeated binding can
/// never violate the composite primary key regardless of what it hands us.
fn contributor_bindings(contributors: &[ImporterContributor]) -> Vec<(String, String)> {
    let mut bindings: Vec<(String, String)> = Vec::with_capacity(contributors.len());
    for contributor in contributors {
        let (role, _) = normalize_contributor_role(contributor.role.as_deref());
        let binding = (contributor.name.clone(), role);
        if !bindings.contains(&binding) {
            bindings.push(binding);
        }
    }
    bindings
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
pub async fn import_file(
    path: String,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    import_one(&path, state.inner()).await
}

/// Import a single file. Shared by `import_file` and `import_files`.
#[tracing::instrument(skip_all, fields(path = %path), ret, err)]
async fn import_one(path: &str, state: &AppState) -> Result<ImportOutcome, ImportError> {
    let file_path = std::path::PathBuf::from(path);
    let extension = file_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (meta, source_bytes) = match importer_source_for_extension(&extension) {
        FileImporterSource::NativeEpub => (
            EpubImporter
                .read_metadata(path.to_string())
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
    // Fail closed before reading file bytes (persist_import revalidates to derive canonical ISBNs).
    validate_importer_meta(&meta)?;

    // Content hash for dedup. Remote imports reuse the exact bytes served
    // through `fs_read`; native parsing still opens the file separately.
    let bytes = match source_bytes {
        Some(bytes) => bytes,
        None => fs_err::tokio::read(&file_path)
            .await
            .map_err(|e| ImportError::new("io", format!("reading file: {e}")))?,
    };

    let outcome = persist_import(
        &state.db,
        &state.covers_dir,
        &meta,
        &bytes,
        &file_path,
        &extension,
    )
    .await?;

    // Surface a fresh edition in search; add_edition currently rebuilds the
    // whole index (see livtet-search write path comment). A duplicate import
    // wrote nothing, so there is nothing new to index.
    if !outcome.duplicate {
        let edition_id = outcome
            .edition_id
            .parse::<DbId>()
            .map_err(|error| ImportError::new("index", error))?;
        let db = state.db.db_conn();
        let guard = state.search_index.read().await;
        if let Some(index) = guard.as_ref() {
            index
                .add_edition(&db, edition_id)
                .await
                .map_err(|e| ImportError::new("index", e))?;
        }
    }

    Ok(outcome)
}

/// Persist the catalog rows for an already-parsed importer record and write the
/// cover to disk. Hash dedup and every row insert share one transaction; a hash
/// match returns the pre-existing edition without writing anything. Kept free of
/// Tauri state so it can be unit-tested against a `TestDb`.
#[tracing::instrument(skip_all, fields(path = %file_path.display()), ret, err)]
async fn persist_import(
    db: &livtet_core::data::SharedState,
    covers_dir: &camino::Utf8Path,
    meta: &ImporterMeta,
    bytes: &[u8],
    file_path: &std::path::Path,
    extension: &str,
) -> Result<ImportOutcome, ImportError> {
    let (isbn_strings, other_identifiers) = validate_importer_meta(meta)?;
    let file_hash = hex::encode(sha2::Sha256::digest(bytes));

    let conn = db.db_conn();

    // Dedup: identical bytes already imported?
    if let Some(existing) = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::FileHash.eq(&file_hash))
        .one(&conn)
        .await?
    {
        let edition = editions::Entity::find_by_id(existing.edition_id)
            .one(&conn)
            .await?;
        let (work_id, title) = edition
            .map(|e| (e.work_id.to_string(), e.title.unwrap_or_default()))
            .unwrap_or_default();
        return Ok(ImportOutcome {
            work_id,
            edition_id: existing.edition_id.to_string(),
            title,
            isbns: isbn_strings,
            duplicate: true,
        });
    }

    let title = meta.title.clone();
    let txn = conn.begin().await?;

    let work_id = DbId::new();
    let language_id = resolve_language(&txn, meta.language.as_deref()).await?;

    let now = now_primitive();
    works::ActiveModel {
        id: Set(work_id),
        title: Set(title.clone()),
        description: Set(meta.description.clone()),
        sort_title: Set(meta.title_sort.clone()),
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
    // ADR-0008 path convention: {data_dir}/data/covers/{inventory_id}/cover.{ext},
    // where data_dir is {dirs data dir}/{BUNDLE_ID} and covers_dir already
    // resolves to its `data/covers` child (see `Paths::new`).
    let cover_path = cover.as_ref().map(|(mime, _)| {
        covers_dir
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
        format_id: Set(format_id_for_extension(extension)),
        language_id: Set(language_id),
        notes: Set(None),
        description: Set(meta.description.clone()),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    // Deduped by `(name, normalized_role)` so a repeated importer binding can
    // never violate the `edition_authors` composite primary key.
    for (name, role) in contributor_bindings(&meta.contributors) {
        let author_id = upsert_author(&txn, &name).await?;
        if !is_core_contributor_role(&role) {
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

    for subject in unique_preserving_order(&meta.subjects) {
        let subject_id = upsert_subject(&txn, &subject).await?;
        edition_subjects::ActiveModel {
            edition_id: Set(edition_id),
            subject_id: Set(subject_id),
        }
        .insert(&txn)
        .await?;
    }

    for (value, kind) in edition_identifier_rows(&isbn_strings, &other_identifiers) {
        let identifier_id = upsert_identifier(&txn, &value, kind).await?;
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
        file_format: Set((!extension.is_empty()).then(|| extension.to_string())),
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
                .one(&conn)
                .await?
                .ok_or_else(|| ImportError::new("database", "inventory row vanished post-commit"))?
                .into();
            model.cover_path = Set(None);
            model.update(&conn).await?;
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

#[tauri::command]
#[tracing::instrument(skip_all, ret, fields(count = paths.len()))]
#[specta::specta]
/// Import several files sequentially, emitting `import://batch` and
/// `import://file` progress events and returning the aggregate result.
pub async fn import_files(paths: Vec<String>, app: AppHandle) -> ImportBatchResult {
    // Tauri rejects async commands that borrow state and return a non-`Result`,
    // so the state is fetched from the handle instead of taken as a parameter.
    let state = app.state::<AppState>();
    let total = paths.len() as i32;
    emit_batch(
        &app,
        ImportBatchProgress {
            phase: ImportPhase::Started,
            total,
            result: None,
        },
    );

    let mut files = Vec::with_capacity(paths.len());
    for (position, path) in paths.into_iter().enumerate() {
        let index = position as i32;
        emit_file(
            &app,
            ImportFileProgress {
                index,
                total,
                path: path.clone(),
                phase: ImportPhase::Started,
                outcome: None,
                error: None,
            },
        );

        let outcome = import_one(&path, state.inner()).await;
        let file = file_result(path, outcome);
        emit_file(
            &app,
            ImportFileProgress {
                index,
                total,
                path: file.path.clone(),
                phase: ImportPhase::Finished,
                outcome: file.outcome.clone(),
                error: file.error.clone(),
            },
        );
        files.push(file);
    }

    let batch = batch_result(files);
    emit_batch(
        &app,
        ImportBatchProgress {
            phase: ImportPhase::Finished,
            total,
            result: Some(batch.clone()),
        },
    );
    batch
}

#[cfg(test)]
mod tests {
    use livtet_core::data::entities::{
        edition_authors, edition_identifiers, edition_subjects, editions, identifiers, works,
    };
    use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
    use livtet_core::data::{Kind, TestDb};
    use livtet_importer::{ImporterContributor, ImporterMeta};
    use livtet_types::DbId;

    use super::{
        FileImporterSource, ImportError, ImportFileResult, ImportOutcome, batch_result,
        contributor_bindings, file_result, format_id_for_extension, identifier_kind,
        importer_source_for_extension, normalize_contributor_role, persist_import,
        validate_importer_meta,
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
            title_sort: None,
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
    }

    #[test]
    fn importer_metadata_accepts_empty_isbns() {
        let mut record = valid_record();
        record.isbns.clear();

        assert_eq!(
            validate_importer_meta(&record).expect("ISBNs are optional"),
            (Vec::<String>::new(), Vec::<String>::new())
        );
    }

    #[test]
    fn importer_metadata_demotes_a_bad_isbn_to_other_identifiers() {
        let mut record = valid_record();
        record.isbns = vec!["not-an-isbn".to_string()];

        assert_eq!(
            validate_importer_meta(&record).expect("a bad ISBN must be demoted"),
            (Vec::<String>::new(), vec!["not-an-isbn".to_string()])
        );
    }

    #[test]
    fn identifier_kinds_are_derived_from_the_value() {
        let cases = [
            ("urn:uuid:372c43e0-812a-4ab2-8076-84c7b1c474af", "uuid"),
            ("372c43e0-812a-4ab2-8076-84c7b1c474af", "uuid"),
            ("B0CR977BQH", "asin"),
            ("urn:oclc:12345", "other"),
        ];
        for (value, kind) in cases {
            assert_eq!(identifier_kind(value), kind, "{value}");
        }
    }

    #[test]
    fn importer_isbns_are_canonicalized_before_persistence() {
        let mut record = valid_record();
        record.isbns = vec!["978-1-78478-060-9".to_string(), "0-306-40615-2".to_string()];

        assert_eq!(
            validate_importer_meta(&record).expect("ISBN variants must canonicalize"),
            (
                vec!["9781784780609".to_string(), "9780306406157".to_string()],
                Vec::<String>::new()
            )
        );
    }

    fn sample_outcome(duplicate: bool) -> ImportOutcome {
        ImportOutcome {
            work_id: "w".to_string(),
            edition_id: "e".to_string(),
            title: "Title".to_string(),
            isbns: Vec::new(),
            duplicate,
        }
    }

    #[test]
    fn file_result_carries_exactly_one_side() {
        let ok = file_result("a.epub".to_string(), Ok(sample_outcome(false)));
        assert!(ok.outcome.is_some());
        assert!(ok.error.is_none());

        let err = file_result(
            "b.epub".to_string(),
            Err(ImportError::new("parse", "bad metadata")),
        );
        assert!(err.outcome.is_none());
        assert_eq!(err.error.as_ref().unwrap().code, "parse");
    }

    #[test]
    fn batch_result_counts_each_category() {
        let files = vec![
            file_result("a.epub".to_string(), Ok(sample_outcome(false))),
            file_result("b.epub".to_string(), Ok(sample_outcome(true))),
            file_result("c.epub".to_string(), Ok(sample_outcome(false))),
            file_result(
                "d.epub".to_string(),
                Err(ImportError::new("unsupported", "no importer")),
            ),
        ];

        let batch = batch_result(files);
        assert_eq!(batch.imported, 2);
        assert_eq!(batch.duplicated, 1);
        assert_eq!(batch.failed, 1);
        assert_eq!(batch.files.len(), 4);
        assert_eq!(batch.files[0].path, "a.epub");
    }

    #[test]
    fn empty_input_is_an_empty_batch() {
        let batch = batch_result(Vec::<ImportFileResult>::new());
        assert_eq!((batch.imported, batch.duplicated, batch.failed), (0, 0, 0));
        assert!(batch.files.is_empty());
    }

    fn contributor(name: &str, role: Option<&str>) -> ImporterContributor {
        ImporterContributor {
            name: name.to_string(),
            role: role.map(str::to_string),
            file_as: None,
        }
    }

    /// The `(name, role)` bindings produced for the given `(name, role)` input.
    fn bindings(pairs: &[(&str, Option<&str>)]) -> Vec<(String, String)> {
        let contributors: Vec<ImporterContributor> = pairs
            .iter()
            .map(|(name, role)| contributor(name, *role))
            .collect();
        contributor_bindings(&contributors)
    }

    fn importable_record(title: &str, title_sort: Option<&str>) -> ImporterMeta {
        ImporterMeta {
            title: title.to_string(),
            title_sort: title_sort.map(str::to_string),
            contributors: vec![contributor("Susana M. Morris", Some("aut"))],
            isbns: vec!["9781784780609".to_string(), "9780306406157".to_string()],
            other_identifiers: Vec::new(),
            publisher: None,
            language: None,
            published: None,
            description: None,
            subjects: Vec::new(),
            cover: None,
        }
    }

    fn temporary_covers_dir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("covers temp dir");
        let path = camino::Utf8Path::from_path(dir.path())
            .expect("utf8 temp path")
            .to_path_buf();
        (dir, path)
    }

    /// Scaffolding shared by the `persist_import` tests: an isolated database,
    /// its state, and a temporary covers directory. Both temp dirs are returned
    /// so the caller keeps them alive for the test's duration.
    async fn persist_fixture() -> (
        TestDb,
        livtet_core::data::SharedState,
        tempfile::TempDir,
        camino::Utf8PathBuf,
    ) {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let (covers, covers_dir) = temporary_covers_dir();
        (test_db, state, covers, covers_dir)
    }

    #[test]
    fn contributor_bindings_dedupes_same_name_and_role() {
        assert_eq!(
            bindings(&[
                ("Susana M. Morris", Some("aut")),
                ("Susana M. Morris", Some("aut")),
            ]),
            vec![("Susana M. Morris".to_string(), "aut".to_string())]
        );
    }

    #[test]
    fn contributor_bindings_keeps_distinct_roles() {
        assert_eq!(
            bindings(&[
                ("Susana M. Morris", Some("aut")),
                ("Susana M. Morris", Some("ill")),
            ]),
            vec![
                ("Susana M. Morris".to_string(), "aut".to_string()),
                ("Susana M. Morris".to_string(), "ill".to_string()),
            ]
        );
    }

    #[test]
    fn contributor_bindings_blank_role_defaults_to_aut() {
        assert_eq!(
            bindings(&[("Susana M. Morris", None), ("Susana M. Morris", Some("  ")),]),
            vec![("Susana M. Morris".to_string(), "aut".to_string())]
        );
    }

    #[test]
    fn contributor_bindings_preserves_order() {
        assert_eq!(
            bindings(&[
                ("First", Some("aut")),
                ("Second", Some("edt")),
                ("Third", Some("ill")),
            ]),
            vec![
                ("First".to_string(), "aut".to_string()),
                ("Second".to_string(), "edt".to_string()),
                ("Third".to_string(), "ill".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn persist_import_records_metadata_and_sort_title() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", Some("Morris, Susana M."));

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"epub bytes",
            std::path::Path::new("/books/positive-obsession.epub"),
            "epub",
        )
        .await
        .expect("persist import");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");

        let author_rows = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition authors");
        assert_eq!(author_rows.len(), 1);

        let work_id = outcome.work_id.parse::<DbId>().expect("work id");
        let work = works::Entity::find_by_id(work_id)
            .one(&conn)
            .await
            .expect("work query")
            .expect("work present");
        assert_eq!(work.sort_title.as_deref(), Some("Morris, Susana M."));

        let identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition identifiers");
        assert_eq!(identifier_rows.len(), 2);
    }

    #[tokio::test]
    async fn persist_import_dedupes_duplicate_contributor_bindings() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.contributors = vec![
            contributor("Susana M. Morris", Some("aut")),
            contributor("Susana M. Morris", Some("aut")),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"duplicate binding bytes",
            std::path::Path::new("/books/duplicate.epub"),
            "epub",
        )
        .await
        .expect("duplicate bindings must persist");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let author_rows = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition authors");
        assert_eq!(author_rows.len(), 1);
    }

    #[tokio::test]
    async fn persist_import_dedupes_duplicate_subjects() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.subjects = vec![
            "Fiction".to_string(),
            "Fiction".to_string(),
            " Fiction ".to_string(),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"duplicate subject bytes",
            std::path::Path::new("/books/duplicate-subjects.epub"),
            "epub",
        )
        .await
        .expect("duplicate subjects must persist");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let subject_rows = edition_subjects::Entity::find()
            .filter(edition_subjects::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition subjects");
        assert_eq!(subject_rows.len(), 1);
    }

    #[tokio::test]
    async fn persist_import_dedupes_duplicate_isbns() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.isbns = vec!["9781784780609".to_string(), "978-1-78478-060-9".to_string()];

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"duplicate isbn bytes",
            std::path::Path::new("/books/duplicate-isbns.epub"),
            "epub",
        )
        .await
        .expect("duplicate isbns must persist");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.isbns, vec!["9781784780609".to_string()]);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition identifiers");
        assert_eq!(identifier_rows.len(), 1);
    }

    #[tokio::test]
    async fn persist_import_persists_non_isbn_identifiers() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("No ISBN Edition", None);
        meta.isbns.clear();
        meta.other_identifiers = vec![
            "urn:uuid:372c43e0-812a-4ab2-8076-84c7b1c474af".to_string(),
            "B0CR977BQH".to_string(),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"non-isbn identifier bytes",
            std::path::Path::new("/books/non-isbn-identifiers.epub"),
            "epub",
        )
        .await
        .expect("editions without an ISBN must persist");
        assert!(!outcome.duplicate);
        assert!(outcome.isbns.is_empty());

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let edition_identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition identifiers");
        assert_eq!(edition_identifier_rows.len(), 2);

        let uuid = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq("urn:uuid:372c43e0-812a-4ab2-8076-84c7b1c474af"))
            .one(&conn)
            .await
            .expect("uuid query")
            .expect("uuid identifier");
        assert_eq!(uuid.kind, "uuid");

        let asin = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq("B0CR977BQH"))
            .one(&conn)
            .await
            .expect("asin query")
            .expect("asin identifier");
        assert_eq!(asin.kind, "asin");
    }

    #[tokio::test]
    async fn persist_import_without_any_identifier_succeeds() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("Identifierless Edition", None);
        meta.isbns.clear();
        meta.other_identifiers.clear();

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"identifierless bytes",
            std::path::Path::new("/books/identifierless.epub"),
            "epub",
        )
        .await
        .expect("editions without any identifier must persist");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let edition = editions::Entity::find_by_id(edition_id)
            .one(&conn)
            .await
            .expect("edition query")
            .expect("edition present");
        assert_eq!(edition.id, edition_id);

        let edition_identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition identifiers");
        assert!(edition_identifier_rows.is_empty());
    }

    #[tokio::test]
    async fn persist_import_dedupes_identifier_values() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let mut meta = importable_record("Duplicate Identifier Edition", None);
        meta.isbns = vec!["not-an-isbn".to_string()];
        meta.other_identifiers = vec!["not-an-isbn".to_string()];

        let outcome = persist_import(
            &state,
            &covers_dir,
            &meta,
            b"duplicate identifier bytes",
            std::path::Path::new("/books/duplicate-identifier.epub"),
            "epub",
        )
        .await
        .expect("duplicate identifier values must persist once");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let edition_identifier_rows = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition identifiers");
        assert_eq!(edition_identifier_rows.len(), 1);

        let identifier_rows = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq("not-an-isbn"))
            .all(&conn)
            .await
            .expect("identifiers");
        assert_eq!(identifier_rows.len(), 1);
    }

    #[tokio::test]
    async fn persist_import_dedupes_identical_file_hash() {
        let (_db, state, _covers, covers_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", None);
        let bytes = b"identical epub bytes";

        let first = persist_import(
            &state,
            &covers_dir,
            &meta,
            bytes,
            std::path::Path::new("/books/first.epub"),
            "epub",
        )
        .await
        .expect("first import");
        assert!(!first.duplicate);

        let second = persist_import(
            &state,
            &covers_dir,
            &meta,
            bytes,
            std::path::Path::new("/books/second.epub"),
            "epub",
        )
        .await
        .expect("second import");
        assert!(second.duplicate);
        assert_eq!(second.edition_id, first.edition_id);

        let conn = state.db_conn();
        let edition_rows = editions::Entity::find().all(&conn).await.expect("editions");
        assert_eq!(edition_rows.len(), 1);

        let author_rows = edition_authors::Entity::find()
            .all(&conn)
            .await
            .expect("edition authors");
        assert_eq!(author_rows.len(), 1);
    }
}
