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

use serde::{Deserialize, Serialize};
use sha2::Digest;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager, State};

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_core::data::{
    entities::{
        authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
        edition_subjects, editions, formats, identifiers, languages, publishers, subjects, works,
    },
    orm::{
        ActiveModelTrait, ColumnTrait, DatabaseConnection, DatabaseTransaction, EntityTrait,
        QueryFilter, Set, TransactionTrait,
    },
};
use livtet_core::shelf::ReadingStatus;
use livtet_importer::{
    AudiobookImporter, EpubImporter, Importer, ImporterContributor, ImporterMeta, MobiImporter,
    PdfImporter,
};
use livtet_types::{
    CommonLanguages, DbId, FormatMetadataSchema, Isbn, KnownFormats, Urn, now_primitive,
};

use super::{
    catalog::{EditionFile, FileStatus},
    importers::remote_importer_metadata,
};
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
    /// Stable machine code: `parse` | `database` | `index` | `io` | `importer` | `network` | `unsupported`.
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

/// How an imported file is stored in the library (`books_dir`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    /// Symlink the source into the library (default). Falls back to a copy
    /// where the platform refuses symlinks.
    #[default]
    Link,
    /// Duplicate the file bytes into the library.
    Copy,
}

/// Where and how an imported file is stored in the library.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LibraryTarget<'a> {
    pub(crate) dir: &'a camino::Utf8Path,
    pub(crate) mode: ImportMode,
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

/// Filesystem name for a library-owned file: `{hash}-{original_filename}`.
/// Only the final component of `source` is used, so the store can never escape
/// `books_dir`. The name is length-capped for `NAME_MAX` (the hash prefix is
/// kept); an empty source name falls back to `book.{ext}`.
fn library_store_name(file_hash: &str, source: &camino::Utf8Path, extension: &str) -> String {
    const MAX_STORE_NAME_LEN: usize = 200;
    let raw = source.file_name().map(str::to_owned).unwrap_or_default();
    let base = match raw.trim() {
        "" if extension.is_empty() => "book".to_string(),
        "" => format!("book.{extension}"),
        name => name.to_string(),
    };
    let mut name = format!("{file_hash}-{base}");
    // `pop` removes a whole char, so truncation stays on a char boundary.
    while name.len() > MAX_STORE_NAME_LEN {
        name.pop();
    }
    name
}

#[cfg(unix)]
fn create_symlink(source: &camino::Utf8Path, dest: &camino::Utf8Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, dest)
}

#[cfg(windows)]
fn create_symlink(source: &camino::Utf8Path, dest: &camino::Utf8Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, dest)
}

#[cfg(not(any(unix, windows)))]
fn create_symlink(_source: &camino::Utf8Path, _dest: &camino::Utf8Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks unsupported on this platform",
    ))
}

fn write_copy(dest: &camino::Utf8Path, bytes: &[u8]) -> Result<(), ImportError> {
    fs_err::write(dest, bytes).map_err(|e| ImportError::new("io", format!("copying file: {e}")))
}

/// Create the library-owned file at `dest` from `source`, returning the
/// effective mode. A refused symlink (e.g. default Windows) falls back to a
/// byte copy with a warning; any other failure fails the import. An existing
/// entry at `dest` is replaced: the store name is hash-prefixed, so it can
/// only name identical content (or a leftover from a crashed import).
#[tracing::instrument(skip(bytes), fields(dest = %dest), ret, err)]
fn materialize_library_file(
    dest: &camino::Utf8Path,
    source: &camino::Utf8Path,
    bytes: &[u8],
    mode: ImportMode,
) -> Result<ImportMode, ImportError> {
    if let Some(parent) = dest.parent() {
        fs_err::create_dir_all(parent)
            .map_err(|e| ImportError::new("io", format!("creating library dir: {e}")))?;
    }
    if dest.exists() || std::fs::symlink_metadata(dest).is_ok() {
        std::fs::remove_file(dest)
            .map_err(|e| ImportError::new("io", format!("replacing library file: {e}")))?;
    }
    match mode {
        ImportMode::Link => match create_symlink(source, dest) {
            Ok(()) => Ok(ImportMode::Link),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
                ) =>
            {
                tracing::warn!(
                    %error,
                    source = %source,
                    %dest,
                    "symlink refused; copying into the library instead"
                );
                write_copy(dest, bytes).map(|()| ImportMode::Copy)
            }
            Err(error) => Err(ImportError::new("io", format!("linking file: {error}"))),
        },
        ImportMode::Copy => write_copy(dest, bytes).map(|()| ImportMode::Copy),
    }
}

/// File extension for a cover MIME type.
fn cover_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "img",
    }
}

/// Write cover bytes to `path`, creating its parent directory.
///
/// Callers record the path in the database, so a failure here means "null the
/// path", not "fail the write": a cover is re-derivable, a catalog row is not.
async fn write_cover_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let path = camino::Utf8Path::new(path);
    if let Some(parent) = path.parent() {
        fs_err::tokio::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    fs_err::tokio::write(path, bytes)
        .await
        .map_err(|e| e.to_string())
}

/// Download a cover for a virtual book, returning the bytes and the file
/// extension derived from the response `Content-Type`.
///
/// Every failure (non-http scheme, network error, non-2xx, non-image type,
/// empty body) logs and yields `None`: a missing cover never blocks creating
/// the book.
async fn fetch_cover_image(http: &reqwest::Client, url: &str) -> Option<(Vec<u8>, &'static str)> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        tracing::debug!(%url, "cover URL is not http(s); skipping the cover");
        return None;
    }

    let response = match http.get(url).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, %url, "cover fetch failed; skipping the cover");
            return None;
        }
    };
    if !response.status().is_success() {
        tracing::warn!(
            status = %response.status(),
            %url,
            "cover fetch returned an error status; skipping the cover"
        );
        return None;
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim())
        .unwrap_or_default();
    let extension = cover_extension(content_type);
    if extension == "img" {
        tracing::warn!(
            %url,
            content_type,
            "cover is not a supported image type; skipping the cover"
        );
        return None;
    }

    match response.bytes().await {
        Ok(bytes) if bytes.is_empty() => {
            tracing::warn!(%url, "cover body was empty; skipping the cover");
            None
        }
        Ok(bytes) => Some((bytes.to_vec(), extension)),
        Err(error) => {
            tracing::warn!(%error, %url, "reading the cover body failed; skipping the cover");
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileImporterSource {
    NativeEpub,
    NativeMobi,
    NativeAudio,
    NativePdf,
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
    matches!(role, "aut" | "edt" | "trl" | "ill" | "nrt")
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
    } else if extension.eq_ignore_ascii_case("azw3") {
        Some(KnownFormats::Azw3.into())
    } else if extension.eq_ignore_ascii_case("azw") {
        Some(KnownFormats::Mobi.into())
    } else if extension.eq_ignore_ascii_case("pdf") {
        Some(KnownFormats::Pdf.into())
    } else if extension.eq_ignore_ascii_case("m4b") || extension.eq_ignore_ascii_case("m4a") {
        Some(KnownFormats::Audiobook.into())
    } else {
        None
    }
}

fn importer_source_for_extension(extension: &str) -> FileImporterSource {
    if extension.eq_ignore_ascii_case("epub") {
        FileImporterSource::NativeEpub
    } else if extension.eq_ignore_ascii_case("azw3") || extension.eq_ignore_ascii_case("azw") {
        FileImporterSource::NativeMobi
    } else if extension.eq_ignore_ascii_case("m4b") || extension.eq_ignore_ascii_case("m4a") {
        FileImporterSource::NativeAudio
    } else if extension.eq_ignore_ascii_case("pdf") {
        FileImporterSource::NativePdf
    } else {
        FileImporterSource::Remote
    }
}

/// Validate per-edition format metadata against the edition format's schema.
///
/// `None` passes through, except for the audiobook format, which must carry
/// `duration_seconds` and `chapters`. A value for an extension with no known
/// catalog format, a missing format row, or a schema violation fails closed.
async fn resolve_format_metadata(
    conn: &DatabaseConnection,
    extension: &str,
    format_metadata: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, ImportError> {
    let format_id = format_id_for_extension(extension);
    let Some(value) = format_metadata else {
        if format_id == Some(KnownFormats::Audiobook.into()) {
            return Err(ImportError::new(
                "importer",
                "audiobook imports must carry duration and chapters",
            ));
        }
        return Ok(None);
    };
    let Some(format_id) = format_id else {
        return Err(ImportError::new(
            "importer",
            "format metadata requires a known catalog format",
        ));
    };
    let format = formats::Entity::find_by_id(format_id)
        .one(conn)
        .await?
        .ok_or_else(|| ImportError::new("importer", "catalog format is missing"))?;
    let schema: FormatMetadataSchema = serde_json::from_value(format.metadata_schema.clone())
        .map_err(|error| ImportError::new("importer", format!("invalid format schema: {error}")))?;
    schema.validate(&value).map_err(|error| {
        ImportError::new("importer", format!("invalid format metadata: {error}"))
    })?;
    Ok(Some(value))
}

#[tauri::command]
#[specta::specta]
pub async fn import_file(
    path: String,
    mode: ImportMode,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    import_one(&path, mode, state.inner()).await
}

/// The slice of [`AppState`] the import pipeline reads. Kept free of Tauri
/// types so [`import_one_inner`] and [`import_batch`] can be unit-tested.
pub(crate) struct ImportContext<'a> {
    pub(crate) db: &'a livtet_core::data::SharedState,
    pub(crate) covers_dir: &'a camino::Utf8Path,
    pub(crate) books_dir: &'a camino::Utf8Path,
    pub(crate) search_index: &'a tokio::sync::RwLock<Option<livtet_core::search::SearchIndex>>,
    pub(crate) plugin_host_path: &'a camino::Utf8Path,
    pub(crate) plugin_host_config: &'a camino::Utf8Path,
    pub(crate) plugins_dir: &'a camino::Utf8Path,
}

impl<'a> From<&'a AppState> for ImportContext<'a> {
    fn from(state: &'a AppState) -> Self {
        Self {
            db: &state.db,
            covers_dir: &state.covers_dir,
            books_dir: &state.books_dir,
            search_index: &state.search_index,
            plugin_host_path: &state.plugin_host_path,
            plugin_host_config: &state.plugin_host_config,
            plugins_dir: &state.plugins_dir,
        }
    }
}

/// Import a single file from app state. Used by `import_file` and the OPDS
/// acquisition handoff; see [`import_one_inner`].
pub(crate) async fn import_one(
    path: &str,
    mode: ImportMode,
    state: &AppState,
) -> Result<ImportOutcome, ImportError> {
    import_one_inner(path, mode, &ImportContext::from(state)).await
}

/// Import a single file: select the importer for its extension, fail closed on
/// its metadata, persist the catalog rows, and index a fresh edition.
#[tracing::instrument(skip_all, fields(path = %path), ret, err)]
pub(crate) async fn import_one_inner(
    path: &str,
    mode: ImportMode,
    ctx: &ImportContext<'_>,
) -> Result<ImportOutcome, ImportError> {
    let file_path = camino::Utf8PathBuf::from(path);
    let extension = file_path
        .extension()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let (meta, source_bytes) = match importer_source_for_extension(&extension) {
        FileImporterSource::NativeEpub => (
            EpubImporter
                .read_metadata(path.to_string())
                .map_err(|error| ImportError::new("parse", error))?,
            None,
        ),
        FileImporterSource::NativeMobi => (
            MobiImporter
                .read_metadata(path.to_string())
                .map_err(|error| ImportError::new("parse", error))?,
            None,
        ),
        FileImporterSource::NativeAudio => (
            AudiobookImporter
                .read_metadata(path.to_string())
                .map_err(|error| ImportError::new("parse", error))?,
            None,
        ),
        FileImporterSource::NativePdf => (
            PdfImporter
                .read_metadata(path.to_string())
                .map_err(|error| ImportError::new("parse", error))?,
            None,
        ),
        FileImporterSource::Remote => {
            let read = remote_importer_metadata(
                ctx.plugin_host_path.to_path_buf(),
                ctx.plugin_host_config.to_path_buf(),
                ctx.plugins_dir.to_path_buf(),
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
        ctx.db,
        ctx.covers_dir,
        LibraryTarget {
            dir: ctx.books_dir,
            mode,
        },
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
        let db = ctx.db.db_conn();
        let guard = ctx.search_index.read().await;
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
#[tracing::instrument(skip_all, fields(path = %file_path), ret, err)]
pub(crate) async fn persist_import(
    db: &livtet_core::data::SharedState,
    covers_dir: &camino::Utf8Path,
    target: LibraryTarget<'_>,
    meta: &ImporterMeta,
    bytes: &[u8],
    file_path: &camino::Utf8Path,
    extension: &str,
) -> Result<ImportOutcome, ImportError> {
    let (isbn_strings, other_identifiers) = validate_importer_meta(meta)?;
    let file_hash = hex::encode(sha2::Sha256::digest(bytes));

    let conn = db.db_conn();
    // Fail closed on format metadata before any bytes are read or rows written.
    let format_metadata =
        resolve_format_metadata(&conn, extension, meta.format_metadata.clone()).await?;

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

    // The library owns its file reference: materialize the link/copy before any
    // row is written, so a storage failure fails the whole import. A hash match
    // above already returned, so this path always names fresh content.
    let library_path = target
        .dir
        .join(library_store_name(&file_hash, file_path, extension));
    materialize_library_file(&library_path, file_path, bytes, target.mode)?;

    let committed = async {
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
            format_metadata: Set(format_metadata.clone()),
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
            file_path: Set(Some(library_path.to_string())),
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
        Ok::<_, ImportError>((title, work_id, edition_id, cover, cover_path))
    }
    .await;
    // Fail closed: a failed transaction must not leave a library file behind.
    let (title, work_id, edition_id, cover, cover_path) = match committed {
        Ok(values) => values,
        Err(error) => {
            let _ = std::fs::remove_file(&library_path);
            return Err(error);
        }
    };

    // Cover bytes go to disk post-commit; a failed write nulls the path
    // (DB stays authoritative, file is re-derivable).
    if let (Some((_, data)), Some(path)) = (&cover, &cover_path)
        && let Err(err) = write_cover_file(path, data).await
    {
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

    Ok(ImportOutcome {
        work_id: work_id.to_string(),
        edition_id: edition_id.to_string(),
        title,
        isbns: isbn_strings,
        duplicate: false,
    })
}

/// One contributor on a manually created virtual book.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VirtualAuthor {
    pub name: String,
    /// MARC relator code; empty defaults to `aut`.
    #[serde(default)]
    pub role: String,
}

/// Catalog metadata for a book that exists only in the database (no file on
/// disk). Created by [`create_virtual_book`].
///
/// `language_id` and `format_id` are `formats.id` / `languages.id` values the
/// caller picks from `filter_options`; the command fails closed on an unknown
/// id.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct VirtualBookInput {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<VirtualAuthor>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub language_id: Option<DbId>,
    #[serde(default)]
    pub format_id: Option<DbId>,
    /// Free-form identifier values; ISBNs are canonicalized, the rest are
    /// stored as `uuid` / `asin` / `other`.
    #[serde(default)]
    pub identifiers: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Optional cover image URL (http/https only). Fetched at creation, written
    /// under `covers_dir/<inventory_id>/cover.<ext>`, and recorded on
    /// `digital_inventory.cover_path`. A failed fetch creates the book without
    /// a cover instead of failing.
    #[serde(default)]
    pub cover_image_url: Option<String>,
}

/// Whether a virtual book must carry a named author.
///
/// The manual form and the MCP tool require one. A book imported from an ATProto
/// shelf may have no contributor names in the book index, and refusing it would
/// lose the shelf entry the user asked to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorRequirement {
    Required,
    Optional,
}

/// Write a virtual book: catalog rows plus a `digital_inventory` row with no
/// `file_path`, so the edition is searchable and browsable but has no on-disk
/// copy. A title is always required; `authors` decides whether a named author is
/// too. Kept free of Tauri state so it can be unit-tested against a `TestDb`.
#[tracing::instrument(skip_all, ret, err)]
pub(crate) async fn create_virtual_book_inner(
    db: &livtet_core::data::SharedState,
    covers_dir: &camino::Utf8Path,
    http: &reqwest::Client,
    input: VirtualBookInput,
    authors: AuthorRequirement,
) -> Result<ImportOutcome, ImportError> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(ImportError::new("invalid", "a virtual book needs a title"));
    }
    let author_bindings: Vec<(String, String)> = input
        .authors
        .iter()
        .filter(|author| !author.name.trim().is_empty())
        .map(|author| {
            let (role, _) = normalize_contributor_role(Some(author.role.as_str()));
            (author.name.trim().to_string(), role)
        })
        .collect();
    if author_bindings.is_empty() && authors == AuthorRequirement::Required {
        return Err(ImportError::new(
            "invalid",
            "a virtual book needs at least one author",
        ));
    }

    // Split identifiers into canonical ISBNs and non-ISBN values, mirroring
    // `validate_importer_meta`: an ISBN-shaped value that fails to parse is
    // demoted rather than failing the write.
    let mut canonical_isbns = Vec::new();
    let mut other_identifiers = Vec::new();
    for raw in &input.identifiers {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        match Isbn::parse(trimmed) {
            Ok(canonical) => canonical_isbns.push(canonical.as_str().to_string()),
            Err(_) => other_identifiers.push(trimmed.to_string()),
        }
    }
    let isbn_strings = unique_preserving_order(&canonical_isbns);
    let other_identifiers = unique_preserving_order(&other_identifiers);

    let conn = db.db_conn();

    // Resolve the chosen format/language, failing closed on an unknown id.
    let format_name = match input.format_id {
        Some(format_id) => Some(
            formats::Entity::find_by_id(format_id)
                .one(&conn)
                .await?
                .map(|format| format.name)
                .ok_or_else(|| ImportError::new("invalid", "unknown format"))?,
        ),
        None => None,
    };
    if let Some(language_id) = input.language_id {
        let exists = languages::Entity::find_by_id(language_id)
            .one(&conn)
            .await?
            .is_some();
        if !exists {
            return Err(ImportError::new("invalid", "unknown language"));
        }
    }

    let now = now_primitive();
    let work_id = DbId::new();
    let edition_id = DbId::new();
    let inventory_id = DbId::new();

    let cover = match input
        .cover_image_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        Some(url) => fetch_cover_image(http, url).await,
        None => None,
    };
    let mut cover_path = cover.as_ref().map(|(_, extension)| {
        covers_dir
            .join(inventory_id.to_string())
            .join(format!("cover.{extension}"))
            .to_string()
    });
    // Cover bytes land before the rows: the insert must be authoritative about
    // whether the path exists. A failed write degrades to "no cover".
    if let (Some((bytes, _)), Some(path)) = (&cover, &cover_path)
        && let Err(err) = write_cover_file(path, bytes).await
    {
        tracing::warn!(%err, %path, "cover write failed; creating the book without a cover");
        cover_path = None;
    }

    let txn = conn.begin().await?;
    works::ActiveModel {
        id: Set(work_id),
        title: Set(title.clone()),
        description: Set(input.description.clone()),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(input.language_id),
        preferred_edition_id: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some(title.clone())),
        published_date: Set(None),
        format_id: Set(input.format_id),
        language_id: Set(input.language_id),
        notes: Set(input.notes.clone()),
        description: Set(input.description.clone()),
        format_metadata: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    for (name, role) in author_bindings {
        let author_id = upsert_author(&txn, &name).await?;
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set(role),
        }
        .insert(&txn)
        .await?;
    }

    if let Some(publisher) = input
        .publisher
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let publisher_id = upsert_publisher(&txn, publisher).await?;
        edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
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

    // The `digital_inventory.file_format` column is NOT NULL with an `EPUB`
    // default; a virtual book keeps a row so the edition stays 1:1 with its
    // inventory, but `file_path` is NULL, which the search/badge layers read as
    // "no file on disk".
    digital_inventory::ActiveModel {
        id: Set(inventory_id),
        edition_id: Set(edition_id),
        file_path: Set(None),
        cover_path: Set(cover_path.clone()),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(None),
        file_size_bytes: Set(None),
        file_format: Set(Some(format_name.unwrap_or_else(|| "unknown".to_string()))),
        notes: Set(None),
        added_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&txn)
    .await?;

    // Fail closed: a failed transaction must not leave a cover file behind.
    if let Err(error) = txn.commit().await {
        if let Some(path) = &cover_path {
            let _ = std::fs::remove_file(path);
        }
        return Err(error.into());
    }

    Ok(ImportOutcome {
        work_id: work_id.to_string(),
        edition_id: edition_id.to_string(),
        title,
        isbns: isbn_strings,
        duplicate: false,
    })
}

/// Create a virtual book: catalog metadata stored in the database with no
/// on-disk file. The new edition is added to the search index so it appears in
/// the library immediately.
#[tauri::command]
#[specta::specta]
pub async fn create_virtual_book(
    input: VirtualBookInput,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    create_virtual_book_and_index(
        &state.db,
        &state.covers_dir,
        &state.opds_http,
        &state.search_index,
        input,
        AuthorRequirement::Required,
    )
    .await
}

/// Write a virtual book and add it to the search index, so it appears in the
/// library immediately. Shared by the IPC command and the MCP bridge
/// (`crate::mcp`), which reaches the same code without a Tauri state.
pub(crate) async fn create_virtual_book_and_index(
    db: &livtet_core::data::SharedState,
    covers_dir: &camino::Utf8Path,
    http: &reqwest::Client,
    search_index: &tokio::sync::RwLock<Option<livtet_core::search::SearchIndex>>,
    input: VirtualBookInput,
    authors: AuthorRequirement,
) -> Result<ImportOutcome, ImportError> {
    let outcome = create_virtual_book_inner(db, covers_dir, http, input, authors).await?;
    let edition_id = outcome
        .edition_id
        .parse::<DbId>()
        .map_err(|error| ImportError::new("index", error))?;
    let conn = db.db_conn();
    let guard = search_index.read().await;
    if let Some(index) = guard.as_ref() {
        index
            .add_edition(&conn, edition_id)
            .await
            .map_err(|error| ImportError::new("index", error))?;
    }
    Ok(outcome)
}

/// One edition summary from the book index search (`searchEditions`).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct BiblioEdition {
    /// `at://` record URI; absent only if the index omits it.
    pub uri: Option<String>,
    pub title: String,
    pub cover_image_url: Option<String>,
    pub published_year: Option<i32>,
    pub language: Option<String>,
    /// Contributors, with roles already normalized to MARC relator codes.
    pub authors: Vec<BiblioContributorSummary>,
    pub identifiers: Vec<BiblioIdentifierSummary>,
}

/// One contributor on a biblio edition summary.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct BiblioContributorSummary {
    pub name: String,
    /// MARC relator code (`aut`, `edt`, `trl`, …).
    pub role: String,
}

/// One external identifier on a biblio edition summary (e.g. OpenLibrary).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct BiblioIdentifierSummary {
    pub resource: String,
    pub uri: String,
}

#[derive(Debug, Deserialize)]
struct BiblioEditionSearchResponse {
    #[serde(default)]
    items: Vec<BiblioEditionRef>,
}

#[derive(Debug, Deserialize)]
struct BiblioEditionRef {
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "coverImageUrl")]
    cover_image_url: Option<String>,
    #[serde(default, rename = "publishedYear")]
    published_year: Option<i32>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    contributors: Vec<BiblioContributorRef>,
    #[serde(default)]
    identifiers: Vec<BiblioIdentifierRef>,
}

#[derive(Debug, Deserialize)]
struct BiblioContributorRef {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BiblioIdentifierRef {
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    uri: Option<String>,
}

/// Map a biblio contributor role to a MARC relator code the catalog stores.
fn biblio_role_to_marc(role: Option<&str>) -> String {
    match role
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "editor" | "edt" => "edt",
        "translator" | "trl" => "trl",
        "illustrator" | "ill" => "ill",
        "narrator" | "nrt" => "nrt",
        _ => "aut",
    }
    .to_string()
}

/// Fold one raw search item into the shape the drawer renders, dropping items
/// with no title and contributors/identifiers with missing fields.
fn map_biblio_edition(item: BiblioEditionRef) -> Option<BiblioEdition> {
    let title = item.title.filter(|title| !title.trim().is_empty())?;
    let authors = item
        .contributors
        .into_iter()
        .filter_map(|contributor| {
            let name = contributor.name.filter(|name| !name.trim().is_empty())?;
            Some(BiblioContributorSummary {
                name,
                role: biblio_role_to_marc(contributor.role.as_deref()),
            })
        })
        .collect();
    let identifiers = item
        .identifiers
        .into_iter()
        .filter_map(|identifier| {
            Some(BiblioIdentifierSummary {
                resource: identifier.resource?,
                uri: identifier.uri?,
            })
        })
        .collect();
    Some(BiblioEdition {
        uri: item.uri,
        title,
        cover_image_url: item.cover_image_url,
        published_year: item.published_year,
        language: item.language,
        authors,
        identifiers,
    })
}

/// Query the book index's `searchEditions` XRPC for editions matching `query`.
/// A blank query returns no results without hitting the network. Kept free of
/// Tauri state so it can be unit-tested with an injected client.
#[tracing::instrument(skip(http), fields(query = %query, limit), ret, err)]
pub(crate) async fn search_biblio_editions_inner(
    http: &reqwest::Client,
    query: &str,
    limit: i32,
) -> Result<Vec<BiblioEdition>, ImportError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "{}/xrpc/{APPVIEW_EDITIONS_NSID}",
        crate::atproto::APPVIEW_URL
    );
    let limit = limit.clamp(1, 50).to_string();
    let response = http
        .get(&url)
        .query(&[("q", query), ("limit", limit.as_str())])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| ImportError::new("network", format!("could not reach the book index: {e}")))?;
    if !response.status().is_success() {
        return Err(ImportError::new(
            "network",
            format!("the book index answered {}", response.status()),
        ));
    }
    let found: BiblioEditionSearchResponse = response.json().await.map_err(|e| {
        ImportError::new(
            "network",
            format!("the book index returned an unreadable response: {e}"),
        )
    })?;
    Ok(found
        .items
        .into_iter()
        .filter_map(map_biblio_edition)
        .collect())
}

/// Search the book index (`biblio.livtet.olamaelcu.net`) for editions. Powers
/// the virtual-book drawer's search tab.
#[tauri::command]
#[specta::specta]
pub async fn search_biblio_editions(
    query: String,
    limit: Option<i32>,
    state: State<'_, AppState>,
) -> Result<Vec<BiblioEdition>, ImportError> {
    search_biblio_editions_inner(&state.opds_http, &query, limit.unwrap_or(20)).await
}

#[tauri::command]
#[tracing::instrument(skip_all, ret, fields(count = paths.len()))]
#[specta::specta]
/// Import several files sequentially, emitting `import://batch` and
/// `import://file` progress events and returning the aggregate result.
pub async fn import_files(
    paths: Vec<String>,
    mode: ImportMode,
    app: AppHandle,
) -> ImportBatchResult {
    // Tauri rejects async commands that borrow state and return a non-`Result`,
    // so the state is fetched from the handle instead of taken as a parameter.
    let state = app.state::<AppState>();
    import_batch(
        paths,
        mode,
        &ImportContext::from(state.inner()),
        |payload| emit_batch(&app, payload),
        |payload| emit_file(&app, payload),
    )
    .await
}

/// Import `paths` sequentially, reporting batch and per-file progress through
/// `on_batch` / `on_file`, and return the aggregate result.
pub(crate) async fn import_batch(
    paths: Vec<String>,
    mode: ImportMode,
    ctx: &ImportContext<'_>,
    mut on_batch: impl FnMut(ImportBatchProgress),
    mut on_file: impl FnMut(ImportFileProgress),
) -> ImportBatchResult {
    let total = paths.len() as i32;
    on_batch(ImportBatchProgress {
        phase: ImportPhase::Started,
        total,
        result: None,
    });

    let mut files = Vec::with_capacity(paths.len());
    for (position, path) in paths.into_iter().enumerate() {
        let index = position as i32;
        on_file(ImportFileProgress {
            index,
            total,
            path: path.clone(),
            phase: ImportPhase::Started,
            outcome: None,
            error: None,
        });

        let outcome = import_one_inner(&path, mode, ctx).await;
        let file = file_result(path, outcome);
        on_file(ImportFileProgress {
            index,
            total,
            path: file.path.clone(),
            phase: ImportPhase::Finished,
            outcome: file.outcome.clone(),
            error: file.error.clone(),
        });
        files.push(file);
    }

    let batch = batch_result(files);
    on_batch(ImportBatchProgress {
        phase: ImportPhase::Finished,
        total,
        result: Some(batch.clone()),
    });
    batch
}

/// Whether `path` names a symlink (never follows it).
fn is_symlink_path(path: &str) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
}

/// Whether `path` lives under `dir`, lexically and without following links.
/// `ParentDir` components are rejected, so `books/../…` cannot pass.
fn is_inside_dir(path: &str, dir: &camino::Utf8Path) -> bool {
    let candidate = camino::Utf8Path::new(path);
    candidate.starts_with(dir)
        && !candidate
            .components()
            .any(|component| matches!(component, camino::Utf8Component::ParentDir))
}

/// Repoint the library file of `edition_id` at `source`, accepting the file
/// and updating the edition's file identity. The entry's existing storage kind
/// is preserved (symlink stays symlink); without a prior file, link. Only a
/// previous file inside `books_dir` is removed — legacy source paths are
/// never deleted. Kept free of Tauri state so it can be unit-tested.
#[tracing::instrument(skip(conn), fields(edition_id = %edition_id, path = %source), ret, err)]
async fn relink_edition(
    conn: &DatabaseConnection,
    books_dir: &camino::Utf8Path,
    edition_id: DbId,
    source: &camino::Utf8Path,
) -> Result<EditionFile, ImportError> {
    let inventory = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(conn)
        .await?
        .ok_or_else(|| ImportError::new("database", "no file recorded for this edition"))?;

    let bytes = fs_err::tokio::read(source)
        .await
        .map_err(|e| ImportError::new("io", format!("reading file: {e}")))?;
    let extension = source
        .extension()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let file_hash = hex::encode(sha2::Sha256::digest(&bytes));

    let mode = match inventory.file_path.as_deref() {
        Some(previous) if !is_symlink_path(previous) => ImportMode::Copy,
        _ => ImportMode::Link,
    };

    let library_path = books_dir.join(library_store_name(&file_hash, source, &extension));
    materialize_library_file(&library_path, source, &bytes, mode)?;

    let previous = inventory.file_path.clone();
    let mut model: digital_inventory::ActiveModel = inventory.into();
    model.file_path = Set(Some(library_path.to_string()));
    model.file_hash = Set(Some(file_hash));
    model.file_size_bytes = Set(Some(bytes.len() as i64));
    // `file_format` is NOT NULL in the schema: an extensionless source keeps
    // the recorded format rather than failing the update.
    if !extension.is_empty() {
        model.file_format = Set(Some(extension));
    }
    model.updated_at = Set(Some(now_primitive()));
    let updated = model.update(conn).await?;

    // Only drop the previous library file once the row points elsewhere, so a
    // failed update never leaves the edition referencing a deleted file.
    if let Some(previous) = previous.as_deref()
        && previous != library_path.as_str()
        && is_inside_dir(previous, books_dir)
    {
        let _ = std::fs::remove_file(previous);
    }

    Ok(EditionFile {
        file_status: Some(FileStatus::Ok),
        file_path: updated.file_path,
        file_format: updated.file_format,
        file_size_bytes: updated.file_size_bytes.map(|size| size as f64),
        cover_path: updated.cover_path,
        blurhash: updated.blurhash,
        dominant_color: updated.dominant_color,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn relink_edition_file(
    edition_id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<EditionFile, ImportError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|error| ImportError::new("database", error))?;
    relink_edition(
        &state.db.db_conn(),
        &state.books_dir,
        id,
        camino::Utf8Path::new(path.as_str()),
    )
    .await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use base64::{Engine, engine::general_purpose::STANDARD};
    use livtet_core::data::entities::{
        authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
        edition_subjects, editions, identifiers, languages, publishers, subjects, works,
    };
    use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
    use livtet_core::data::{Kind, TestDb};
    use livtet_importer::{
        ImporterContributor, ImporterCover, ImporterMeta, ImporterPublicationDate,
    };
    use livtet_types::DbId;

    use super::{AuthorRequirement, VirtualAuthor, VirtualBookInput, create_virtual_book_inner};
    use super::{
        FileImporterSource, ImportBatchProgress, ImportContext, ImportError, ImportFileProgress,
        ImportFileResult, ImportMode, ImportOutcome, ImportPhase, LibraryTarget, batch_result,
        contributor_bindings, cover_extension, file_result, format_id_for_extension,
        identifier_kind, import_batch, import_one_inner, importer_source_for_extension,
        is_inside_dir, library_store_name, materialize_library_file, normalize_contributor_role,
        persist_import, relink_edition, validate_importer_meta,
    };
    use crate::commands::catalog::FileStatus;
    use livtet_core::data::orm::{ActiveModelTrait, Set};
    use sha2::Digest;

    #[test]
    fn epub_extension_uses_the_native_importer() {
        assert!(matches!(
            importer_source_for_extension("EPUB"),
            FileImporterSource::NativeEpub
        ));
    }

    #[test]
    fn azw3_and_azw_extensions_use_the_native_mobi_importer() {
        assert!(matches!(
            importer_source_for_extension("AZW3"),
            FileImporterSource::NativeMobi
        ));
        assert!(matches!(
            importer_source_for_extension("azw"),
            FileImporterSource::NativeMobi
        ));
    }

    #[test]
    fn m4b_and_m4a_extensions_use_the_native_audio_importer() {
        assert!(matches!(
            importer_source_for_extension("m4b"),
            FileImporterSource::NativeAudio
        ));
        assert!(matches!(
            importer_source_for_extension("M4A"),
            FileImporterSource::NativeAudio
        ));
    }

    #[test]
    fn pdf_extension_uses_the_native_pdf_importer() {
        assert!(matches!(
            importer_source_for_extension("PDF"),
            FileImporterSource::NativePdf
        ));
    }

    #[test]
    fn unknown_extensions_use_remote_importers() {
        assert!(matches!(
            importer_source_for_extension("mobi"),
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
        assert_eq!(
            normalize_contributor_role(Some("NRT")),
            ("nrt".to_string(), true)
        );
    }

    #[test]
    fn extensions_map_to_known_catalog_formats() {
        assert_eq!(
            format_id_for_extension("epub"),
            Some(livtet_types::KnownFormats::Epub.into())
        );
        assert_eq!(
            format_id_for_extension("azw3"),
            Some(livtet_types::KnownFormats::Azw3.into())
        );
        assert_eq!(
            format_id_for_extension("AZW"),
            Some(livtet_types::KnownFormats::Mobi.into())
        );
        assert_eq!(
            format_id_for_extension("PDF"),
            Some(livtet_types::KnownFormats::Pdf.into())
        );
        assert_eq!(
            format_id_for_extension("m4b"),
            Some(livtet_types::KnownFormats::Audiobook.into())
        );
        assert_eq!(
            format_id_for_extension("M4A"),
            Some(livtet_types::KnownFormats::Audiobook.into())
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
            format_metadata: None,
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
            format_metadata: None,
        }
    }

    fn temporary_dir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = camino::Utf8Path::from_path(dir.path())
            .expect("utf8 temp path")
            .to_path_buf();
        (dir, path)
    }

    fn temporary_covers_dir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        temporary_dir()
    }

    fn temporary_books_dir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        temporary_dir()
    }

    /// Scaffolding shared by the `persist_import` tests: an isolated database,
    /// its state, and a temporary covers directory. Both temp dirs are returned
    /// so the caller keeps them alive for the test's duration.
    async fn persist_fixture() -> (
        TestDb,
        livtet_core::data::SharedState,
        tempfile::TempDir,
        camino::Utf8PathBuf,
        tempfile::TempDir,
        camino::Utf8PathBuf,
    ) {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let (covers, covers_dir) = temporary_covers_dir();
        let (books, books_dir) = temporary_books_dir();
        (test_db, state, covers, covers_dir, books, books_dir)
    }

    #[tokio::test]
    async fn create_virtual_book_writes_an_edition_without_a_file() {
        let (_db, state, _covers, covers_dir, _books, _books_dir) = persist_fixture().await;
        let input = VirtualBookInput {
            title: "The Great Gatsby".to_string(),
            authors: vec![VirtualAuthor {
                name: "F. Scott Fitzgerald".to_string(),
                role: "aut".to_string(),
            }],
            publisher: Some("Scribner".to_string()),
            language_id: None,
            format_id: None,
            identifiers: vec!["9780743273565".to_string(), "abc".to_string()],
            description: Some("A novel".to_string()),
            notes: None,
            cover_image_url: None,
        };

        let outcome = create_virtual_book_inner(
            &state,
            &covers_dir,
            &reqwest::Client::new(),
            input,
            AuthorRequirement::Required,
        )
        .await
        .expect("create virtual book");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.title, "The Great Gatsby");

        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();
        let edition = editions::Entity::find_by_id(edition_id)
            .one(&conn)
            .await
            .expect("query edition")
            .expect("edition present");
        assert_eq!(edition.title.as_deref(), Some("The Great Gatsby"));
        assert!(edition.format_id.is_none());

        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("query inventory")
            .expect("inventory row");
        assert!(
            inventory.file_path.is_none(),
            "virtual book has no file path"
        );
        assert!(inventory.file_hash.is_none());
        assert!(inventory.file_size_bytes.is_none());
        assert!(
            inventory.cover_path.is_none(),
            "no cover URL, no cover path"
        );

        let author_links = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("query authors");
        assert_eq!(author_links.len(), 1);
        let publisher_links = edition_publishers::Entity::find()
            .filter(edition_publishers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("query publishers");
        assert_eq!(publisher_links.len(), 1);
        let identifier_links = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("query identifiers");
        assert_eq!(
            identifier_links.len(),
            2,
            "canonical ISBN + other identifier"
        );
    }

    #[tokio::test]
    async fn create_virtual_book_requires_a_title_and_an_author() {
        let (_db, state, _covers, covers_dir, _books, _books_dir) = persist_fixture().await;
        let http = reqwest::Client::new();
        let blank_title = VirtualBookInput {
            title: "   ".to_string(),
            authors: vec![VirtualAuthor {
                name: "A".to_string(),
                role: "aut".to_string(),
            }],
            publisher: None,
            language_id: None,
            format_id: None,
            identifiers: vec![],
            description: None,
            notes: None,
            cover_image_url: None,
        };
        assert!(
            create_virtual_book_inner(
                &state,
                &covers_dir,
                &http,
                blank_title,
                AuthorRequirement::Required
            )
            .await
            .is_err()
        );

        let no_author = VirtualBookInput {
            title: "A Title".to_string(),
            authors: vec![],
            publisher: None,
            language_id: None,
            format_id: None,
            identifiers: vec![],
            description: None,
            notes: None,
            cover_image_url: None,
        };
        assert!(
            create_virtual_book_inner(
                &state,
                &covers_dir,
                &http,
                no_author,
                AuthorRequirement::Required
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn an_authorless_virtual_book_is_allowed_when_the_caller_says_so() {
        // A shelved book can have no contributor names in the book index; the
        // shelf import passes `Optional` so the book is kept without inventing
        // an author.
        let (_db, state, _covers, covers_dir, _books, _books_dir) = persist_fixture().await;
        let input = VirtualBookInput {
            title: "Was Hinduism Invented?".to_string(),
            authors: vec![],
            publisher: None,
            language_id: None,
            format_id: None,
            identifiers: vec![],
            description: None,
            notes: None,
            cover_image_url: None,
        };

        let outcome = create_virtual_book_inner(
            &state,
            &covers_dir,
            &reqwest::Client::new(),
            input,
            AuthorRequirement::Optional,
        )
        .await
        .expect("authorless virtual book");
        assert!(!outcome.duplicate);

        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();
        let links = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("query authors");
        assert!(
            links.is_empty(),
            "no placeholder author is invented for a book the index cannot attribute"
        );
    }

    /// A one-pixel stand-in for a cover; only the bytes matter here.
    const COVER_BYTES: &[u8] = b"\x89PNG\r\n\x1a\nvirtual";

    #[derive(Clone)]
    struct CoverState {
        body: &'static [u8],
    }

    #[poem::handler]
    fn cover_fixture(
        req: &poem::Request,
        poem::web::Data(state): poem::web::Data<&CoverState>,
    ) -> poem::Response {
        if req.uri().path() == "/cover.png" {
            poem::Response::builder()
                .content_type("image/png")
                .body(state.body.to_vec())
        } else {
            poem::Response::builder()
                .status(poem::http::StatusCode::NOT_FOUND)
                .body(Vec::new())
        }
    }

    /// Origin of a loopback server that serves [`COVER_BYTES`] at `/cover.png`
    /// and answers 404 everywhere else. The serving task is detached and dies
    /// with the test runtime.
    fn cover_server() -> String {
        use poem::EndpointExt;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let acceptor = poem::listener::TcpAcceptor::from_std(listener).unwrap();
        tokio::spawn(
            poem::Server::new_with_acceptor(acceptor)
                .run(cover_fixture.data(CoverState { body: COVER_BYTES })),
        );
        origin
    }

    fn virtual_book_input(title: &str, cover_image_url: Option<String>) -> VirtualBookInput {
        VirtualBookInput {
            title: title.to_string(),
            authors: vec![VirtualAuthor {
                name: "Ursula K. Le Guin".to_string(),
                role: "aut".to_string(),
            }],
            publisher: None,
            language_id: None,
            format_id: None,
            identifiers: vec![],
            description: None,
            notes: None,
            cover_image_url,
        }
    }

    #[tokio::test]
    async fn create_virtual_book_persists_a_fetched_cover() {
        let (_db, state, _covers, covers_dir, _books, _books_dir) = persist_fixture().await;
        let origin = cover_server();
        let input = virtual_book_input("A Wizard of Earthsea", Some(format!("{origin}/cover.png")));

        let outcome = create_virtual_book_inner(
            &state,
            &covers_dir,
            &reqwest::Client::new(),
            input,
            AuthorRequirement::Required,
        )
        .await
        .expect("create virtual book with a cover");

        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("query inventory")
            .expect("inventory row");
        assert!(inventory.file_path.is_none(), "the book stays virtual");

        let expected = covers_dir
            .join(inventory.id.to_string())
            .join("cover.png")
            .to_string();
        assert_eq!(inventory.cover_path.as_deref(), Some(expected.as_str()));
        assert_eq!(
            fs_err::read(&expected).expect("cover file on disk"),
            COVER_BYTES
        );

        // The query the library grid runs to find covers.
        let covers = crate::commands::catalog::fetch_edition_covers(&conn, &[edition_id])
            .await
            .expect("query edition covers");
        assert_eq!(covers.len(), 1);
        assert_eq!(covers[0].edition_id, edition_id.to_string());
        assert_eq!(covers[0].cover_path, expected);
    }

    #[tokio::test]
    async fn create_virtual_book_survives_a_failed_cover_fetch() {
        let (_db, state, _covers, covers_dir, _books, _books_dir) = persist_fixture().await;
        let origin = cover_server();
        let input = virtual_book_input("The Dispossessed", Some(format!("{origin}/missing.png")));

        let outcome = create_virtual_book_inner(
            &state,
            &covers_dir,
            &reqwest::Client::new(),
            input,
            AuthorRequirement::Required,
        )
        .await
        .expect("the book is created without a cover");

        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("query inventory")
            .expect("inventory row");
        assert!(inventory.cover_path.is_none(), "a 404 leaves no cover path");
        assert!(
            crate::commands::catalog::fetch_edition_covers(&conn, &[edition_id])
                .await
                .expect("query edition covers")
                .is_empty()
        );
    }

    #[test]
    fn map_biblio_edition_normalizes_roles_and_drops_blank_fields() {
        let item = super::BiblioEditionRef {
            uri: Some("at://did:web:biblio/community.lexicon.book.edition/ol.OL1M".to_string()),
            title: Some("Dune".to_string()),
            cover_image_url: Some("https://covers.example/1.jpg".to_string()),
            published_year: Some(1965),
            language: Some("eng".to_string()),
            contributors: vec![
                super::BiblioContributorRef {
                    name: Some("Frank Herbert".to_string()),
                    role: Some("author".to_string()),
                },
                super::BiblioContributorRef {
                    name: Some("   ".to_string()),
                    role: Some("editor".to_string()),
                },
                super::BiblioContributorRef {
                    name: Some("Ed Itor".to_string()),
                    role: Some("editor".to_string()),
                },
            ],
            identifiers: vec![
                super::BiblioIdentifierRef {
                    resource: Some("openlibrary".to_string()),
                    uri: Some("https://openlibrary.org/books/OL1M".to_string()),
                },
                super::BiblioIdentifierRef {
                    resource: None,
                    uri: Some("orphan".to_string()),
                },
            ],
        };

        let mapped = super::map_biblio_edition(item).expect("mapped");
        assert_eq!(mapped.title, "Dune");
        assert_eq!(mapped.published_year, Some(1965));
        assert_eq!(mapped.authors.len(), 2, "blank name dropped");
        assert_eq!(mapped.authors[0].role, "aut");
        assert_eq!(mapped.authors[1].role, "edt");
        assert_eq!(
            mapped.identifiers.len(),
            1,
            "identifier missing resource dropped"
        );
        assert_eq!(mapped.identifiers[0].resource, "openlibrary");
    }

    #[test]
    fn map_biblio_edition_drops_items_without_a_title() {
        let item = super::BiblioEditionRef {
            uri: None,
            title: Some("   ".to_string()),
            cover_image_url: None,
            published_year: None,
            language: None,
            contributors: vec![],
            identifiers: vec![],
        };
        assert!(super::map_biblio_edition(item).is_none());
    }

    #[tokio::test]
    async fn search_biblio_editions_blank_query_skips_the_network() {
        let http = reqwest::Client::new();
        let results = super::search_biblio_editions_inner(&http, "   ", 10)
            .await
            .expect("blank query never touches the network");
        assert!(results.is_empty());
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", Some("Morris, Susana M."));

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"epub bytes",
            camino::Utf8Path::new("/books/positive-obsession.epub"),
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
    async fn persist_import_stores_validated_audiobook_format_metadata() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Memoir", None);
        meta.format_metadata = Some(serde_json::json!({
            "duration_seconds": 7200,
            "chapters": [{"name": "Intro", "audio_start": 0, "audio_end": 300}],
        }));

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"audiobook bytes",
            camino::Utf8Path::new("/books/memoir.m4b"),
            "m4b",
        )
        .await
        .expect("audiobook format metadata persists");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let edition = editions::Entity::find_by_id(edition_id)
            .one(&conn)
            .await
            .expect("edition query")
            .expect("edition present");
        assert_eq!(edition.format_metadata, meta.format_metadata);
    }

    #[tokio::test]
    async fn persist_import_rejects_invalid_audiobook_format_metadata() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Memoir", None);
        meta.format_metadata = Some(serde_json::json!({}));

        persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"invalid audiobook bytes",
            camino::Utf8Path::new("/books/invalid.m4b"),
            "m4b",
        )
        .await
        .expect_err("invalid audiobook format metadata fails closed");
    }

    #[tokio::test]
    async fn persist_import_dedupes_duplicate_contributor_bindings() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.contributors = vec![
            contributor("Susana M. Morris", Some("aut")),
            contributor("Susana M. Morris", Some("aut")),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"duplicate binding bytes",
            camino::Utf8Path::new("/books/duplicate.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.subjects = vec![
            "Fiction".to_string(),
            "Fiction".to_string(),
            " Fiction ".to_string(),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"duplicate subject bytes",
            camino::Utf8Path::new("/books/duplicate-subjects.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Positive Obsession", None);
        meta.isbns = vec!["9781784780609".to_string(), "978-1-78478-060-9".to_string()];

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"duplicate isbn bytes",
            camino::Utf8Path::new("/books/duplicate-isbns.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("No ISBN Edition", None);
        meta.isbns.clear();
        meta.other_identifiers = vec![
            "urn:uuid:372c43e0-812a-4ab2-8076-84c7b1c474af".to_string(),
            "B0CR977BQH".to_string(),
        ];

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"non-isbn identifier bytes",
            camino::Utf8Path::new("/books/non-isbn-identifiers.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Identifierless Edition", None);
        meta.isbns.clear();
        meta.other_identifiers.clear();

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"identifierless bytes",
            camino::Utf8Path::new("/books/identifierless.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Duplicate Identifier Edition", None);
        meta.isbns = vec!["not-an-isbn".to_string()];
        meta.other_identifiers = vec!["not-an-isbn".to_string()];

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"duplicate identifier bytes",
            camino::Utf8Path::new("/books/duplicate-identifier.epub"),
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
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", None);
        let bytes = b"identical epub bytes";

        let first = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            bytes,
            camino::Utf8Path::new("/books/first.epub"),
            "epub",
        )
        .await
        .expect("first import");
        assert!(!first.duplicate);

        let second = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            bytes,
            camino::Utf8Path::new("/books/second.epub"),
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

    #[test]
    fn library_store_name_prefixes_hash_and_keeps_filename() {
        assert_eq!(
            library_store_name(
                "abc123",
                camino::Utf8Path::new("/books/Positive Obsession.epub"),
                "epub"
            ),
            "abc123-Positive Obsession.epub"
        );
    }

    #[test]
    fn library_store_name_falls_back_when_source_has_no_filename() {
        assert_eq!(
            library_store_name("abc123", camino::Utf8Path::new("/"), "epub"),
            "abc123-book.epub"
        );
    }

    #[test]
    fn library_store_name_caps_length_but_keeps_the_hash() {
        let long = "a".repeat(300);
        let source = camino::Utf8PathBuf::from(format!("/books/{long}.epub"));
        let hash = "b".repeat(64);
        let name = library_store_name(&hash, &source, "epub");
        assert!(name.len() <= 200, "{name}");
        assert!(name.starts_with(&hash), "{name}");
    }

    #[test]
    fn materialize_link_points_at_the_source() {
        let root = tempfile::tempdir().expect("temp root");
        let source = camino::Utf8PathBuf::from_path_buf(root.path().join("book.epub"))
            .expect("utf8 temp path");
        fs_err::write(&source, b"book bytes").expect("source");
        let books = camino::Utf8Path::from_path(root.path())
            .expect("utf8 temp path")
            .to_path_buf();
        let dest = books.join("lib").join("hash-book.epub");

        let used = materialize_library_file(&dest, &source, b"book bytes", ImportMode::Link)
            .expect("materialize");
        assert_eq!(used, ImportMode::Link);
        assert_eq!(
            std::fs::read_link(&dest).expect("read link"),
            source.into_std_path_buf()
        );
        assert_eq!(
            fs_err::read(&dest).expect("read through link"),
            b"book bytes"
        );
    }

    #[test]
    fn materialize_copy_writes_exact_bytes() {
        let root = tempfile::tempdir().expect("temp root");
        let source = camino::Utf8PathBuf::from_path_buf(root.path().join("book.epub"))
            .expect("utf8 temp path");
        fs_err::write(&source, b"book bytes").expect("source");
        let books = camino::Utf8Path::from_path(root.path())
            .expect("utf8 temp path")
            .to_path_buf();
        let dest = books.join("hash-book.epub");

        let used = materialize_library_file(&dest, &source, b"book bytes", ImportMode::Copy)
            .expect("materialize");
        assert_eq!(used, ImportMode::Copy);
        assert_eq!(fs_err::read(&dest).expect("read copy"), b"book bytes");
        assert!(
            !std::fs::symlink_metadata(&dest)
                .expect("metadata")
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn persist_import_stores_a_library_owned_link() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", None);
        let source = camino::Utf8Path::new("/books/positive-obsession.epub");

        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"library bytes",
            source,
            "epub",
        )
        .await
        .expect("persist import");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("inventory query")
            .expect("inventory present");
        let stored = inventory.file_path.expect("file path");
        assert!(stored.starts_with(books_dir.as_str()), "{stored}");
        assert!(stored.ends_with("positive-obsession.epub"), "{stored}");
        assert_eq!(std::fs::read_link(&stored).expect("link target"), source);
    }

    #[tokio::test]
    async fn persist_import_duplicate_materializes_no_second_file() {
        let (_db, state, _covers, covers_dir, books, books_dir) = persist_fixture().await;
        let meta = importable_record("Positive Obsession", None);
        let bytes = b"shared library bytes";

        let first = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            bytes,
            camino::Utf8Path::new("/books/first.epub"),
            "epub",
        )
        .await
        .expect("first import");
        assert!(!first.duplicate);

        let second = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            bytes,
            camino::Utf8Path::new("/books/second.epub"),
            "epub",
        )
        .await
        .expect("second import");
        assert!(second.duplicate);

        let entries = std::fs::read_dir(books.path())
            .expect("read books dir")
            .count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn library_ownership_rejects_parent_traversal() {
        let books = camino::Utf8Path::new("/data/books");
        assert!(is_inside_dir("/data/books/hash-name.epub", books));
        assert!(!is_inside_dir("/data/books/../evil.epub", books));
        assert!(!is_inside_dir("/home/user/book.epub", books));
    }

    async fn previously_stored_path(
        conn: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
    ) -> String {
        digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(conn)
            .await
            .expect("inventory query")
            .expect("inventory present")
            .file_path
            .expect("file path")
    }

    #[tokio::test]
    async fn relink_updates_identity_and_preserves_symlink_kind() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Relinked", None);
        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"original bytes",
            camino::Utf8Path::new("/books/original.epub"),
            "epub",
        )
        .await
        .expect("persist import");
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();
        let previous = previously_stored_path(&conn, edition_id).await;

        let outside = tempfile::tempdir().expect("outside dir");
        let new_source =
            camino::Utf8PathBuf::from_path_buf(outside.path().join("replacement.epub"))
                .expect("utf8 temp path");
        fs_err::write(&new_source, b"replacement bytes").expect("write replacement");

        let file = relink_edition(&conn, &books_dir, edition_id, &new_source)
            .await
            .expect("relink");
        assert_eq!(file.file_status, Some(FileStatus::Ok));
        let stored = file.file_path.clone().expect("file path");
        assert!(stored.starts_with(books_dir.as_str()), "{stored}");
        assert!(stored.ends_with("replacement.epub"), "{stored}");
        assert_eq!(
            std::fs::read_link(&stored).expect("link target"),
            new_source.into_std_path_buf()
        );

        // Identity follows the new file.
        let row = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("inventory query")
            .expect("inventory present");
        assert_eq!(
            row.file_hash.as_deref(),
            Some(hex::encode(sha2::Sha256::digest(b"replacement bytes")).as_str())
        );
        assert_eq!(row.file_size_bytes, Some(17));

        // The previous library link is gone.
        assert!(std::fs::symlink_metadata(&previous).is_err(), "{previous}");
    }

    #[tokio::test]
    async fn relink_preserves_copy_kind() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Copied", None);
        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Copy,
            },
            &meta,
            b"copied bytes",
            camino::Utf8Path::new("/books/copied.epub"),
            "epub",
        )
        .await
        .expect("persist import");
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();

        let outside = tempfile::tempdir().expect("outside dir");
        let new_source =
            camino::Utf8PathBuf::from_path_buf(outside.path().join("replacement.epub"))
                .expect("utf8 temp path");
        fs_err::write(&new_source, b"fresh copy bytes").expect("write replacement");

        let file = relink_edition(&conn, &books_dir, edition_id, &new_source)
            .await
            .expect("relink");
        let stored = file.file_path.expect("file path");
        assert_eq!(
            fs_err::read(&stored).expect("read copy"),
            b"fresh copy bytes"
        );
        assert!(
            !std::fs::symlink_metadata(&stored)
                .expect("metadata")
                .file_type()
                .is_symlink()
        );
    }

    #[tokio::test]
    async fn relink_never_deletes_a_legacy_source_path() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Legacy", None);
        let outcome = persist_import(
            &state,
            &covers_dir,
            LibraryTarget {
                dir: &books_dir,
                mode: ImportMode::Link,
            },
            &meta,
            b"legacy bytes",
            camino::Utf8Path::new("/books/legacy.epub"),
            "epub",
        )
        .await
        .expect("persist import");
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let conn = state.db_conn();

        // Simulate a pre-feature row pointing at a real user file.
        let outside = tempfile::tempdir().expect("outside dir");
        let original = camino::Utf8PathBuf::from_path_buf(outside.path().join("precious.epub"))
            .expect("utf8 temp path");
        fs_err::write(&original, b"precious").expect("write original");
        let row = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("inventory query")
            .expect("inventory present");
        let mut model: digital_inventory::ActiveModel = row.into();
        model.file_path = Set(Some(original.as_str().to_owned()));
        model.update(&conn).await.expect("backdate row");

        let new_source = camino::Utf8PathBuf::from_path_buf(outside.path().join("new.epub"))
            .expect("utf8 temp path");
        fs_err::write(&new_source, b"new bytes").expect("write new");
        let file = relink_edition(&conn, &books_dir, edition_id, &new_source)
            .await
            .expect("relink");
        assert!(original.exists(), "legacy source must survive relink");
        assert!(
            file.file_path
                .as_deref()
                .expect("file path")
                .starts_with(books_dir.as_str())
        );
    }

    // ---- Shared helpers for the tests below --------------------------------

    /// Persist `meta` as if it were read from `/books/source.{extension}`,
    /// linking it into `books_dir`. The store name is hash-prefixed, so
    /// distinct `bytes` never collide.
    async fn persist_meta(
        state: &livtet_core::data::SharedState,
        covers_dir: &camino::Utf8Path,
        books_dir: &camino::Utf8Path,
        meta: &ImporterMeta,
        bytes: &[u8],
        extension: &str,
    ) -> Result<ImportOutcome, ImportError> {
        let source = camino::Utf8PathBuf::from(format!("/books/source.{extension}"));
        persist_import(
            state,
            covers_dir,
            LibraryTarget {
                dir: books_dir,
                mode: ImportMode::Link,
            },
            meta,
            bytes,
            &source,
            extension,
        )
        .await
    }

    async fn stored_edition(
        conn: &livtet_core::data::orm::DatabaseConnection,
        outcome: &ImportOutcome,
    ) -> editions::Model {
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        editions::Entity::find_by_id(edition_id)
            .one(conn)
            .await
            .expect("edition query")
            .expect("edition present")
    }

    async fn stored_inventory(
        conn: &livtet_core::data::orm::DatabaseConnection,
        outcome: &ImportOutcome,
    ) -> digital_inventory::Model {
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(conn)
            .await
            .expect("inventory query")
            .expect("inventory present")
    }

    async fn row_count<E: EntityTrait>(conn: &livtet_core::data::orm::DatabaseConnection) -> usize {
        E::find().all(conn).await.expect("count rows").len()
    }

    async fn language_id_for_code(
        conn: &livtet_core::data::orm::DatabaseConnection,
        code: &str,
    ) -> Option<DbId> {
        languages::Entity::find()
            .filter(languages::Column::Code.eq(code))
            .one(conn)
            .await
            .expect("language query")
            .map(|language| language.id)
    }

    fn dir_entries(dir: &camino::Utf8Path) -> usize {
        std::fs::read_dir(dir).expect("read dir").count()
    }

    const PNG_COVER: &[u8] = b"\x89PNG\r\n\x1a\nimport-cover-png";
    const JPEG_COVER: &[u8] = b"\xFF\xD8\xFF\xE0import-cover-jpeg";

    const CONTAINER_XML: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    const LANTERN_METADATA: &str = r#"
    <dc:identifier id="bookid">9780306406157</dc:identifier>
    <dc:title>The Lantern Keeper</dc:title>
    <dc:creator>Ada Quill</dc:creator>
    <dc:publisher>Harbor Press</dc:publisher>
    <dc:language>fr</dc:language>
    <dc:subject>Fiction</dc:subject>
    "#;

    /// Write a minimal EPUB 3 with `metadata` (and optionally a JPEG
    /// `cover-image`) to `dir/name`.
    fn write_epub(
        dir: &camino::Utf8Path,
        name: &str,
        metadata: &str,
        with_cover: bool,
    ) -> camino::Utf8PathBuf {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let cover_item = if with_cover {
            r#"<item id="cover-image" href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#
        } else {
            ""
        };
        let opf = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
{metadata}
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    {cover_item}
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#
        );
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("mimetype", options).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            zip.start_file("META-INF/container.xml", options).unwrap();
            zip.write_all(CONTAINER_XML.as_bytes()).unwrap();
            zip.start_file("OEBPS/content.opf", options).unwrap();
            zip.write_all(opf.as_bytes()).unwrap();
            zip.start_file("OEBPS/chapter.xhtml", options).unwrap();
            zip.write_all(b"<html><body>One</body></html>").unwrap();
            if with_cover {
                zip.start_file("OEBPS/cover.jpg", options).unwrap();
                zip.write_all(JPEG_COVER).unwrap();
            }
            zip.finish().unwrap();
        }
        let path = dir.join(name);
        fs_err::write(&path, cursor.into_inner()).expect("write epub");
        path
    }

    /// Owns everything an [`ImportContext`] borrows. The plugin host path
    /// names a file that does not exist, so any remote import fails to launch.
    struct ImportHarness {
        _db: TestDb,
        state: livtet_core::data::SharedState,
        _covers: tempfile::TempDir,
        covers_dir: camino::Utf8PathBuf,
        _books: tempfile::TempDir,
        books_dir: camino::Utf8PathBuf,
        _sources: tempfile::TempDir,
        sources_dir: camino::Utf8PathBuf,
        missing_host: camino::Utf8PathBuf,
        search_index: tokio::sync::RwLock<Option<livtet_core::search::SearchIndex>>,
        _index_dir: Option<tempfile::TempDir>,
    }

    impl ImportHarness {
        async fn new(with_index: bool) -> Self {
            let (db, state, covers, covers_dir, books, books_dir) = persist_fixture().await;
            let (sources, sources_dir) = temporary_dir();
            let missing_host = sources_dir.join("missing-plugin-host");
            let (index_dir, index) = if with_index {
                let (dir, path) = temporary_dir();
                let index = livtet_core::search::SearchIndex::open(&path).expect("search index");
                (Some(dir), Some(index))
            } else {
                (None, None)
            };
            Self {
                _db: db,
                state,
                _covers: covers,
                covers_dir,
                _books: books,
                books_dir,
                _sources: sources,
                sources_dir,
                missing_host,
                search_index: tokio::sync::RwLock::new(index),
                _index_dir: index_dir,
            }
        }

        fn ctx(&self) -> ImportContext<'_> {
            ImportContext {
                db: &self.state,
                covers_dir: &self.covers_dir,
                books_dir: &self.books_dir,
                search_index: &self.search_index,
                plugin_host_path: &self.missing_host,
                plugin_host_config: &self.missing_host,
                plugins_dir: &self.sources_dir,
            }
        }
    }

    // ---- Error shape --------------------------------------------------------

    #[test]
    fn database_errors_surface_with_the_database_code() {
        let error = ImportError::from(livtet_core::data::orm::DbErr::Custom(
            "disk is full".to_string(),
        ));
        assert_eq!(error.code, "database");
        assert!(error.message.contains("disk is full"), "{}", error.message);

        let rendered = error.to_string();
        assert!(
            rendered.contains("database") && rendered.contains("disk is full"),
            "{rendered}"
        );
    }

    #[test]
    fn batch_result_does_not_count_a_result_without_either_side() {
        let batch = batch_result(vec![ImportFileResult {
            path: "ghost.epub".to_string(),
            outcome: None,
            error: None,
        }]);
        assert_eq!((batch.imported, batch.duplicated, batch.failed), (0, 0, 0));
    }

    // ---- Library file materialization ----------------------------------------

    #[cfg(unix)]
    #[test]
    fn materialize_replaces_stale_and_dangling_entries_at_dest() {
        let (_root, root) = temporary_dir();
        let source = root.join("book.epub");
        fs_err::write(&source, b"fresh bytes").expect("source");
        let lib = root.join("lib");
        fs_err::create_dir_all(&lib).expect("lib dir");

        // A leftover regular file from a crashed import is overwritten.
        let stale = lib.join("hash-book.epub");
        fs_err::write(&stale, b"stale bytes").expect("stale file");
        let used = materialize_library_file(&stale, &source, b"fresh bytes", ImportMode::Copy)
            .expect("replace stale file");
        assert_eq!(used, ImportMode::Copy);
        assert_eq!(fs_err::read(&stale).expect("read copy"), b"fresh bytes");

        // A dangling symlink (`exists()` is false) is replaced, not tripped over.
        let dangling = lib.join("hash-other.epub");
        std::os::unix::fs::symlink(root.join("gone.epub"), &dangling).expect("dangling link");
        let used = materialize_library_file(&dangling, &source, b"fresh bytes", ImportMode::Link)
            .expect("replace dangling link");
        assert_eq!(used, ImportMode::Link);
        assert_eq!(
            std::fs::read_link(&dangling).expect("read link"),
            source.into_std_path_buf()
        );
    }

    // ---- Format metadata gate ----------------------------------------------

    #[tokio::test]
    async fn audiobook_without_format_metadata_fails_closed_before_writing() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Silent Audiobook", None);

        let error = persist_meta(&state, &covers_dir, &books_dir, &meta, b"m4b bytes", "m4b")
            .await
            .expect_err("audiobooks need duration and chapters");
        assert_eq!(error.code, "importer");

        let conn = state.db_conn();
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&books_dir), 0);
    }

    #[tokio::test]
    async fn format_metadata_for_an_unknown_format_fails_closed() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Plain Text", None);
        meta.format_metadata = Some(serde_json::json!({ "pages": 3 }));

        let error = persist_meta(&state, &covers_dir, &books_dir, &meta, b"txt bytes", "txt")
            .await
            .expect_err("metadata without a catalog format is rejected");
        assert_eq!(error.code, "importer");

        let conn = state.db_conn();
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&books_dir), 0);
    }

    // ---- Name-keyed upserts --------------------------------------------------

    #[tokio::test]
    async fn publisher_is_linked_and_reused_across_editions() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Harbor Tales", None);
        meta.publisher = Some("Harbor Press".to_string());

        let first = persist_meta(&state, &covers_dir, &books_dir, &meta, b"first", "epub")
            .await
            .expect("first import");
        let second = persist_meta(&state, &covers_dir, &books_dir, &meta, b"second", "epub")
            .await
            .expect("second import");
        assert!(!second.duplicate);

        let conn = state.db_conn();
        let publisher_rows = publishers::Entity::find()
            .filter(publishers::Column::Name.eq("Harbor Press"))
            .all(&conn)
            .await
            .expect("publishers");
        assert_eq!(publisher_rows.len(), 1);
        let publisher_id = publisher_rows[0].id;

        for outcome in [&first, &second] {
            let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
            let links = edition_publishers::Entity::find()
                .filter(edition_publishers::Column::EditionId.eq(edition_id))
                .all(&conn)
                .await
                .expect("edition publishers");
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].publisher_id, publisher_id);
        }
    }

    #[tokio::test]
    async fn second_edition_reuses_existing_author_subject_and_identifier_rows() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Shared Rows", None);
        meta.isbns = vec!["9781784780609".to_string()];
        meta.subjects = vec!["Lighthouses".to_string()];

        let first = persist_meta(&state, &covers_dir, &books_dir, &meta, b"one", "epub")
            .await
            .expect("first import");
        let second = persist_meta(&state, &covers_dir, &books_dir, &meta, b"two", "epub")
            .await
            .expect("second import");
        assert_ne!(first.edition_id, second.edition_id);

        let conn = state.db_conn();
        let ids = |outcome: &ImportOutcome| outcome.edition_id.parse::<DbId>().expect("id");
        let (first_id, second_id) = (ids(&first), ids(&second));

        let first_authors = linked_author_ids(&conn, first_id).await;
        assert_eq!(first_authors.len(), 1);
        assert_eq!(first_authors, linked_author_ids(&conn, second_id).await);
        let author_rows = authors::Entity::find()
            .filter(authors::Column::Name.eq("Susana M. Morris"))
            .all(&conn)
            .await
            .expect("authors");
        assert_eq!(author_rows.len(), 1);

        let first_subjects = linked_subject_ids(&conn, first_id).await;
        assert_eq!(first_subjects.len(), 1);
        assert_eq!(first_subjects, linked_subject_ids(&conn, second_id).await);
        let subject_rows = subjects::Entity::find()
            .filter(subjects::Column::Name.eq("Lighthouses"))
            .all(&conn)
            .await
            .expect("subjects");
        assert_eq!(subject_rows.len(), 1);

        let first_identifiers = linked_identifier_ids(&conn, first_id).await;
        assert_eq!(first_identifiers.len(), 1);
        assert_eq!(
            first_identifiers,
            linked_identifier_ids(&conn, second_id).await
        );
    }

    async fn linked_author_ids(
        conn: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
    ) -> Vec<DbId> {
        edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(conn)
            .await
            .expect("edition authors")
            .into_iter()
            .map(|row| row.author_id)
            .collect()
    }

    async fn linked_subject_ids(
        conn: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
    ) -> Vec<DbId> {
        edition_subjects::Entity::find()
            .filter(edition_subjects::Column::EditionId.eq(edition_id))
            .all(conn)
            .await
            .expect("edition subjects")
            .into_iter()
            .map(|row| row.subject_id)
            .collect()
    }

    async fn linked_identifier_ids(
        conn: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
    ) -> Vec<DbId> {
        edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .all(conn)
            .await
            .expect("edition identifiers")
            .into_iter()
            .map(|row| row.identifier_id)
            .collect()
    }

    #[tokio::test]
    async fn non_core_contributor_role_is_stored_verbatim() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Anthology", None);
        meta.contributors = vec![
            contributor("Susana M. Morris", Some("aut")),
            contributor("Alice Sheldon", Some("CTB")),
        ];

        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"anthology", "epub")
            .await
            .expect("persist import");

        let conn = state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let mut roles: Vec<String> = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .all(&conn)
            .await
            .expect("edition authors")
            .into_iter()
            .map(|row| row.role)
            .collect();
        roles.sort();
        assert_eq!(roles, vec!["aut".to_string(), "ctb".to_string()]);
    }

    // ---- Language resolution -------------------------------------------------

    #[tokio::test]
    async fn seeded_language_is_reused_for_639_1_and_639_3_codes() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let conn = state.db_conn();
        let english = language_id_for_code(&conn, "en")
            .await
            .expect("English is seeded");
        let languages_before = row_count::<languages::Entity>(&conn).await;

        for (code, bytes) in [("en", b"en".as_slice()), ("eng", b"eng".as_slice())] {
            let mut meta = importable_record("Language Book", None);
            meta.language = Some(code.to_string());
            let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, bytes, "epub")
                .await
                .expect("persist import");

            let edition = stored_edition(&conn, &outcome).await;
            assert_eq!(edition.language_id, Some(english), "{code}");
            let work = works::Entity::find_by_id(edition.work_id)
                .one(&conn)
                .await
                .expect("work query")
                .expect("work present");
            assert_eq!(work.language_id, Some(english), "{code}");
        }
        assert_eq!(
            row_count::<languages::Entity>(&conn).await,
            languages_before
        );
    }

    #[tokio::test]
    async fn unseeded_language_is_created_once_and_then_reused() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let conn = state.db_conn();
        assert_eq!(language_id_for_code(&conn, "ga").await, None);
        let languages_before = row_count::<languages::Entity>(&conn).await;

        let mut meta = importable_record("Irish Book", None);
        meta.language = Some("ga".to_string());
        let first = persist_meta(&state, &covers_dir, &books_dir, &meta, b"ga", "epub")
            .await
            .expect("first import");
        meta.language = Some("gle".to_string());
        let second = persist_meta(&state, &covers_dir, &books_dir, &meta, b"gle", "epub")
            .await
            .expect("second import");

        let irish = languages::Entity::find()
            .filter(languages::Column::Code.eq("ga"))
            .all(&conn)
            .await
            .expect("languages");
        assert_eq!(irish.len(), 1);
        assert_eq!(irish[0].name, "Irish");
        assert_eq!(
            row_count::<languages::Entity>(&conn).await,
            languages_before + 1
        );
        assert_eq!(
            stored_edition(&conn, &first).await.language_id,
            Some(irish[0].id)
        );
        assert_eq!(
            stored_edition(&conn, &second).await.language_id,
            Some(irish[0].id)
        );
    }

    #[tokio::test]
    async fn unrecognized_language_leaves_the_edition_without_a_language() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let conn = state.db_conn();
        let languages_before = row_count::<languages::Entity>(&conn).await;
        let mut meta = importable_record("Mystery Tongue", None);
        meta.language = Some("not-a-language".to_string());

        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"mystery", "epub")
            .await
            .expect("an unknown language must not fail the import");

        assert_eq!(stored_edition(&conn, &outcome).await.language_id, None);
        assert_eq!(
            row_count::<languages::Entity>(&conn).await,
            languages_before
        );
    }

    #[tokio::test]
    #[ignore = "bug: livtet-types at the locked rev (b7be4c6) normalizes `en-US`/`en-GB` to None, so region-tagged EPUB languages are dropped; fixed upstream, needs a livtet-core bump"]
    async fn region_tagged_language_resolves_to_a_seeded_row() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let conn = state.db_conn();
        let english = language_id_for_code(&conn, "en").await.expect("English");
        let uk_english = language_id_for_code(&conn, "en-GB")
            .await
            .expect("UK English is seeded");

        for (code, bytes, expected) in [
            ("en-US", b"en-US".as_slice(), english),
            ("en_US", b"en_US".as_slice(), english),
            ("en-GB", b"en-GB".as_slice(), uk_english),
        ] {
            let mut meta = importable_record("Regional Book", None);
            meta.language = Some(code.to_string());
            let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, bytes, "epub")
                .await
                .expect("persist import");
            assert_eq!(
                stored_edition(&conn, &outcome).await.language_id,
                Some(expected),
                "{code}"
            );
        }
    }

    // ---- Publication dates ---------------------------------------------------

    #[tokio::test]
    async fn published_date_defaults_missing_parts_and_drops_impossible_dates() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let conn = state.db_conn();
        let date = |year: i32, month: time::Month, day: u8| {
            time::Date::from_calendar_date(year, month, day).expect("valid calendar date")
        };
        let cases = [
            (
                2025,
                None,
                None,
                b"year-only".as_slice(),
                Some(date(2025, time::Month::January, 1)),
            ),
            (
                2021,
                Some(6),
                Some(15),
                b"full".as_slice(),
                Some(date(2021, time::Month::June, 15)),
            ),
            (2021, Some(2), Some(30), b"feb-30".as_slice(), None),
        ];

        for (year, month, day, bytes, expected) in cases {
            let mut meta = importable_record("Dated Book", None);
            meta.published = Some(ImporterPublicationDate { year, month, day });
            let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, bytes, "epub")
                .await
                .expect("an impossible date must not fail the import");
            assert_eq!(
                stored_edition(&conn, &outcome).await.published_date,
                expected,
                "{year}-{month:?}-{day:?}"
            );
        }
    }

    // ---- Covers --------------------------------------------------------------

    #[test]
    fn cover_extension_follows_the_mime_type() {
        let cases = [
            ("image/jpeg", "jpg"),
            ("image/png", "png"),
            ("image/webp", "webp"),
            ("image/gif", "gif"),
            ("image/svg+xml", "img"),
            ("application/octet-stream", "img"),
        ];
        for (mime, extension) in cases {
            assert_eq!(cover_extension(mime), extension, "{mime}");
        }
    }

    #[tokio::test]
    async fn cover_is_written_under_the_inventory_directory() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Covered", None);
        meta.cover = Some(ImporterCover {
            mime: "image/png".to_string(),
            data_base64: STANDARD.encode(PNG_COVER),
        });

        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"covered", "epub")
            .await
            .expect("persist import");

        let conn = state.db_conn();
        let inventory = stored_inventory(&conn, &outcome).await;
        let expected = covers_dir.join(inventory.id.to_string()).join("cover.png");
        assert_eq!(inventory.cover_path.as_deref(), Some(expected.as_str()));
        assert_eq!(fs_err::read(&expected).expect("cover file"), PNG_COVER);
    }

    #[tokio::test]
    async fn undecodable_cover_rolls_back_rows_and_library_file() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let mut meta = importable_record("Broken Cover", None);
        meta.cover = Some(ImporterCover {
            mime: "image/png".to_string(),
            data_base64: "@@ not base64 @@".to_string(),
        });

        let error = persist_meta(&state, &covers_dir, &books_dir, &meta, b"broken", "epub")
            .await
            .expect_err("invalid cover data fails the import");
        assert_eq!(error.code, "parse");

        let conn = state.db_conn();
        assert_eq!(row_count::<works::Entity>(&conn).await, 0);
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(row_count::<digital_inventory::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&books_dir), 0, "library file must be removed");
    }

    // fs-err formats tokio I/O errors via `block_in_place`, which needs the
    // multi-thread runtime the app runs on.
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_cover_write_keeps_the_import_and_clears_cover_path() {
        let (_db, state, _covers, _covers_dir, _books, books_dir) = persist_fixture().await;
        // A regular file where the covers directory should be: every cover
        // directory creation under it fails.
        let (_blocker_root, blocker_root) = temporary_dir();
        let covers_dir = blocker_root.join("covers");
        fs_err::write(&covers_dir, b"not a directory").expect("blocker file");

        let mut meta = importable_record("Uncovered", None);
        meta.cover = Some(ImporterCover {
            mime: "image/jpeg".to_string(),
            data_base64: STANDARD.encode(JPEG_COVER),
        });

        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"uncovered", "epub")
            .await
            .expect("a cover write failure must not fail the import");
        assert!(!outcome.duplicate);

        let conn = state.db_conn();
        let inventory = stored_inventory(&conn, &outcome).await;
        assert_eq!(inventory.cover_path, None);
        assert!(inventory.file_path.is_some());
    }

    // ---- Relink edge cases -----------------------------------------------------

    #[tokio::test]
    async fn relink_without_a_recorded_file_is_a_database_error() {
        let (_db, state, _covers, _covers_dir, _books, books_dir) = persist_fixture().await;
        let (_outside, outside) = temporary_dir();
        let source = outside.join("orphan.epub");
        fs_err::write(&source, b"orphan").expect("source");

        let error = relink_edition(&state.db_conn(), &books_dir, DbId::new(), &source)
            .await
            .expect_err("no inventory row to relink");
        assert_eq!(error.code, "database");
        assert_eq!(dir_entries(&books_dir), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn relink_to_an_unreadable_source_leaves_the_entry_untouched() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("Stays Put", None);
        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"stays", "epub")
            .await
            .expect("persist import");
        let conn = state.db_conn();
        let before = stored_inventory(&conn, &outcome).await;

        let (_outside, outside) = temporary_dir();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let error = relink_edition(&conn, &books_dir, edition_id, &outside.join("missing.epub"))
            .await
            .expect_err("missing source cannot be relinked");
        assert_eq!(error.code, "io");

        let after = stored_inventory(&conn, &outcome).await;
        assert_eq!(after.file_path, before.file_path);
        assert_eq!(after.file_hash, before.file_hash);
        let previous = before.file_path.expect("file path");
        assert!(std::fs::symlink_metadata(&previous).is_ok(), "{previous}");
    }

    #[tokio::test]
    async fn relink_to_an_extensionless_source_keeps_the_recorded_format() {
        let (_db, state, _covers, covers_dir, _books, books_dir) = persist_fixture().await;
        let meta = importable_record("No Extension", None);
        let outcome = persist_meta(&state, &covers_dir, &books_dir, &meta, b"noext", "epub")
            .await
            .expect("persist import");
        let conn = state.db_conn();
        let previous = stored_inventory(&conn, &outcome)
            .await
            .file_path
            .expect("file path");

        let (_outside, outside) = temporary_dir();
        let source = outside.join("replacement");
        fs_err::write(&source, b"extensionless bytes").expect("source");
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let file = relink_edition(&conn, &books_dir, edition_id, &source)
            .await
            .expect("an extensionless source must relink");

        // `file_format` is NOT NULL: the recorded format survives.
        assert_eq!(file.file_format.as_deref(), Some("epub"));
        let row = stored_inventory(&conn, &outcome).await;
        assert_eq!(row.file_format.as_deref(), Some("epub"));
        let stored = row.file_path.expect("file path");
        assert_eq!(file.file_path.as_deref(), Some(stored.as_str()));
        assert_eq!(
            fs_err::read(&stored).expect("read through link"),
            b"extensionless bytes"
        );
        assert!(std::fs::symlink_metadata(&previous).is_err(), "{previous}");
    }

    // ---- import_one_inner / import_batch ---------------------------------------

    #[tokio::test]
    async fn native_epub_import_persists_metadata_cover_and_search_entry() {
        let harness = ImportHarness::new(true).await;
        let source = write_epub(&harness.sources_dir, "lantern.epub", LANTERN_METADATA, true);

        let outcome = import_one_inner(source.as_str(), ImportMode::Link, &harness.ctx())
            .await
            .expect("import epub");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.title, "The Lantern Keeper");
        assert_eq!(outcome.isbns, vec!["9780306406157".to_string()]);

        let conn = harness.state.db_conn();
        let edition = stored_edition(&conn, &outcome).await;
        assert_eq!(edition.format_id, format_id_for_extension("epub"));
        assert_eq!(edition.language_id, language_id_for_code(&conn, "fr").await);
        assert!(edition.language_id.is_some());

        let publisher_links = edition_publishers::Entity::find()
            .filter(edition_publishers::Column::EditionId.eq(edition.id))
            .all(&conn)
            .await
            .expect("edition publishers");
        assert_eq!(publisher_links.len(), 1);
        let publisher = publishers::Entity::find_by_id(publisher_links[0].publisher_id)
            .one(&conn)
            .await
            .expect("publisher query")
            .expect("publisher present");
        assert_eq!(publisher.name, "Harbor Press");

        let inventory = stored_inventory(&conn, &outcome).await;
        let stored = inventory.file_path.clone().expect("file path");
        assert!(stored.starts_with(harness.books_dir.as_str()), "{stored}");
        assert_eq!(
            std::fs::read_link(&stored).expect("library link"),
            source.clone().into_std_path_buf()
        );
        assert_eq!(inventory.file_format.as_deref(), Some("epub"));
        let cover_path = harness
            .covers_dir
            .join(inventory.id.to_string())
            .join("cover.jpg");
        assert_eq!(inventory.cover_path.as_deref(), Some(cover_path.as_str()));
        assert_eq!(fs_err::read(&cover_path).expect("cover file"), JPEG_COVER);

        {
            let guard = harness.search_index.read().await;
            let hits = guard
                .as_ref()
                .expect("index")
                .search("Lantern", 10)
                .await
                .expect("search");
            assert!(
                hits.iter()
                    .any(|hit| hit.edition_id.as_deref() == Some(outcome.edition_id.as_str())),
                "fresh import must be searchable"
            );
        }

        // Re-importing the same bytes is a no-op returning the same edition.
        let again = import_one_inner(source.as_str(), ImportMode::Link, &harness.ctx())
            .await
            .expect("re-import epub");
        assert!(again.duplicate);
        assert_eq!(again.edition_id, outcome.edition_id);
        assert_eq!(row_count::<editions::Entity>(&conn).await, 1);
        assert_eq!(dir_entries(&harness.books_dir), 1);
    }

    #[tokio::test]
    async fn unparseable_native_files_fail_with_the_parse_code() {
        let harness = ImportHarness::new(false).await;
        for extension in ["epub", "azw3", "azw", "m4b", "m4a", "pdf"] {
            let source = harness.sources_dir.join(format!("garbage.{extension}"));
            fs_err::write(&source, b"this is not a book in any format").expect("garbage");

            let error = import_one_inner(source.as_str(), ImportMode::Copy, &harness.ctx())
                .await
                .expect_err("garbage must not import");
            assert_eq!(error.code, "parse", "{extension}: {}", error.message);
        }

        let conn = harness.state.db_conn();
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&harness.books_dir), 0);
    }

    #[tokio::test]
    async fn epub_without_a_creator_is_rejected_before_anything_is_stored() {
        let harness = ImportHarness::new(false).await;
        let source = write_epub(
            &harness.sources_dir,
            "anonymous.epub",
            r#"<dc:identifier id="bookid">9780306406157</dc:identifier>
    <dc:title>Anonymous</dc:title>"#,
            false,
        );

        let error = import_one_inner(source.as_str(), ImportMode::Copy, &harness.ctx())
            .await
            .expect_err("a creator is required");
        assert_eq!(error.code, "parse");

        let conn = harness.state.db_conn();
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&harness.books_dir), 0);
    }

    #[tokio::test]
    async fn non_native_extension_is_routed_to_the_remote_importer() {
        let harness = ImportHarness::new(false).await;
        let source = harness.sources_dir.join("notes.xyz");
        fs_err::write(&source, b"remote-only format").expect("source");

        // The native importers would answer `parse`; only the remote path
        // reports a host launch failure as `importer`.
        let error = import_one_inner(source.as_str(), ImportMode::Copy, &harness.ctx())
            .await
            .expect_err("no plugin host is available");
        assert_eq!(error.code, "importer", "{}", error.message);

        let conn = harness.state.db_conn();
        assert_eq!(row_count::<editions::Entity>(&conn).await, 0);
        assert_eq!(dir_entries(&harness.books_dir), 0);
    }

    #[tokio::test]
    async fn batch_reports_ordered_progress_and_aggregates_outcomes() {
        let harness = ImportHarness::new(false).await;
        let good = write_epub(
            &harness.sources_dir,
            "lantern.epub",
            LANTERN_METADATA,
            false,
        );
        let bad = harness.sources_dir.join("broken.epub");
        fs_err::write(&bad, b"not a zip").expect("broken epub");
        let paths = vec![good.to_string(), good.to_string(), bad.to_string()];

        let mut batch_events: Vec<ImportBatchProgress> = Vec::new();
        let mut file_events: Vec<ImportFileProgress> = Vec::new();
        let result = import_batch(
            paths.clone(),
            ImportMode::Copy,
            &harness.ctx(),
            |event| batch_events.push(event),
            |event| file_events.push(event),
        )
        .await;

        assert_eq!(
            (result.imported, result.duplicated, result.failed),
            (1, 1, 1)
        );
        let returned_paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            returned_paths,
            paths.iter().map(String::as_str).collect::<Vec<_>>()
        );
        let first = result.files[0].outcome.as_ref().expect("first imported");
        let second = result.files[1].outcome.as_ref().expect("second duplicated");
        assert!(!first.duplicate);
        assert!(second.duplicate);
        assert_eq!(second.edition_id, first.edition_id);
        assert_eq!(
            result.files[2].error.as_ref().map(|e| e.code.as_str()),
            Some("parse")
        );

        // Batch: started (no result) then finished (carrying the aggregate).
        assert_eq!(batch_events.len(), 2);
        assert!(matches!(batch_events[0].phase, ImportPhase::Started));
        assert_eq!(batch_events[0].total, 3);
        assert!(batch_events[0].result.is_none());
        assert!(matches!(batch_events[1].phase, ImportPhase::Finished));
        let finished = batch_events[1].result.as_ref().expect("final aggregate");
        assert_eq!(
            (finished.imported, finished.duplicated, finished.failed),
            (1, 1, 1)
        );

        // Files: a started/finished pair per path, in order, with the
        // finished event mirroring that file's result.
        assert_eq!(file_events.len(), 6);
        for (position, pair) in file_events.chunks(2).enumerate() {
            let (started, done) = (&pair[0], &pair[1]);
            assert_eq!(started.index, position as i32);
            assert_eq!(done.index, position as i32);
            assert_eq!(started.total, 3);
            assert_eq!(started.path, paths[position]);
            assert_eq!(done.path, paths[position]);
            assert!(matches!(started.phase, ImportPhase::Started));
            assert!(started.outcome.is_none() && started.error.is_none());
            assert!(matches!(done.phase, ImportPhase::Finished));
            assert_eq!(
                done.outcome.as_ref().map(|o| o.duplicate),
                result.files[position].outcome.as_ref().map(|o| o.duplicate)
            );
            assert_eq!(
                done.error.as_ref().map(|e| e.code.clone()),
                result.files[position]
                    .error
                    .as_ref()
                    .map(|e| e.code.clone())
            );
        }
    }
}

/// Outcome of moving a book onto a shelf.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ShelfSync {
    /// `at://` URI of the shelving record now in the account's repository.
    pub record_uri: Option<String>,
    /// Why the record was not published, when it was not.
    pub error: Option<String>,
}

/// Place an edition on a shelf: write the local row, then publish the
/// shelving record (`net.olamaelcu.livtet.biblio.bookShelving`) to the
/// account's own repository.
///
/// The edition reference comes from the book index, which mints the edition
/// records the shelving record points at; the index title is written to the
/// record so it renders without a lookup.
#[tauri::command]
#[specta::specta]
pub async fn sync_shelf_placement(
    edition_id: String,
    shelf_key: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<ShelfSync, ImportError> {
    let status = ReadingStatus::from_key(&shelf_key)
        .ok_or_else(|| ImportError::new("shelf", format!("unknown shelf `{shelf_key}`")))?;
    let app = state.inner();

    let did = match app.atproto.get_current_did().await {
        Ok(did) => did,
        Err(crate::error::AtprotoError::NotSignedIn) => String::new(),
        Err(error) => return Err(ImportError::new("atproto", error)),
    };
    tracing::info!(
        edition_id = %edition_id,
        shelf = status.as_str(),
        signed_in = !did.is_empty(),
        "shelf move requested"
    );

    // 1. Local row. One shelving row per book per account, so a move retargets
    // the existing row instead of adding a shelf-shaped duplicate.
    let conn = app.db.db_conn();
    let local_shelf_uri = format!("shelf:{}", status.as_str());
    livtet_core::shelf::ensure_shelf(&conn, &local_shelf_uri, status.shelf_name())
        .await
        .map_err(|e| ImportError::new("db", format!("creating shelf: {e}")))?;
    let row =
        livtet_core::shelf::upsert_book_shelf(&conn, &did, &local_shelf_uri, &edition_id, status)
            .await
            .map_err(|e| ImportError::new("db", format!("writing shelf row: {e}")))?;
    tracing::info!(
        uri = %row.uri,
        shelf = %row.shelf_uri,
        status = %row.status,
        "shelf row written"
    );

    // 2. Publish. Without a signed-in account or an edition record there is
    // nothing to point the shelving record at, so report rather than guess.
    if did.is_empty() {
        return Ok(ShelfSync {
            record_uri: None,
            error: Some("Sign in to ATProto to publish shelves".to_string()),
        });
    }
    if title.trim().is_empty() {
        return Ok(ShelfSync {
            record_uri: None,
            error: Some("This edition has no title to look up in the book index".to_string()),
        });
    }
    let (book_uri, index_title) = match resolve_edition_record(&app.opds_http, &title).await {
        Ok(found) => found,
        Err(error) => {
            tracing::warn!(%error, title = %title, "could not resolve the edition record");
            return Ok(ShelfSync {
                record_uri: None,
                error: Some(error),
            });
        }
    };

    match app
        .atproto
        .shelve_book(&book_uri, &index_title, status)
        .await
    {
        Ok(shelved) => {
            tracing::info!(
                record = %shelved.record_uri,
                shelf = %shelved.shelf_uri,
                "shelf published to the PDS"
            );
            Ok(ShelfSync {
                record_uri: Some(shelved.record_uri),
                error: None,
            })
        }
        Err(error) => {
            tracing::warn!(%error, "publishing the shelf record failed");
            Ok(ShelfSync {
                record_uri: None,
                error: Some(error.to_string()),
            })
        }
    }
}

/// Read the shelf an edition currently sits on.
#[tauri::command]
#[specta::specta]
/// Read current shelf placement for an edition.
pub async fn get_shelf_placement(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, ImportError> {
    let conn = state.inner().db.db_conn();
    let row = livtet_core::shelf::shelf_of_book(&conn, &edition_id)
        .await
        .map_err(|e| ImportError::new("db", format!("reading shelf: {e}")))?;
    Ok(row.map(|r| r.status))
}

/// The book index that mints edition records.
const APPVIEW_EDITIONS_NSID: &str = "community.lexicon.book.searchEditions";
/// Edition records live in this collection.
const EDITION_COLLECTION: &str = "community.lexicon.book.edition";

#[derive(Deserialize)]
struct EditionSearch {
    items: Vec<EditionHit>,
}

#[derive(Deserialize)]
struct EditionHit {
    uri: String,
    title: Option<String>,
}

/// Look up the edition record for `title` and return it with the index title.
///
/// Matching is exact (case- and whitespace-insensitive): writing a shelving
/// record for a near-match would shelve the wrong book, so a miss is an error
/// the caller shows instead.
pub(crate) async fn resolve_edition_record(
    http: &reqwest::Client,
    title: &str,
) -> Result<(String, String), String> {
    let url = format!(
        "{}/xrpc/{APPVIEW_EDITIONS_NSID}",
        crate::atproto::APPVIEW_URL
    );
    let response = http
        .get(&url)
        .query(&[("q", title), ("limit", "10")])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("could not reach the book index: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "the book index answered {} for `{title}`",
            response.status()
        ));
    }
    let found: EditionSearch = response
        .json()
        .await
        .map_err(|e| format!("the book index returned an unreadable response: {e}"))?;

    let wanted = normalize_title(title);
    let hit = found
        .items
        .into_iter()
        .find(|item| {
            item.uri.contains(&format!("/{EDITION_COLLECTION}/"))
                && item
                    .title
                    .as_deref()
                    .map(normalize_title)
                    .is_some_and(|candidate| candidate == wanted)
        })
        .ok_or_else(|| format!("no edition record titled `{title}` in the book index"))?;

    let found_title = hit.title.unwrap_or_else(|| title.to_string());
    Ok((hit.uri, found_title))
}

/// Case- and whitespace-insensitive title key.
fn normalize_title(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
