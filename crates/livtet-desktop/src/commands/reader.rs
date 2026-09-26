//! Reader window commands: the audiobook descriptor backend and the EPUB
//! streamer/navigator backend behind one dispatching entry point.
//!
//! Audiobooks play in a dedicated `/reader/[editionId]` window backed by the
//! loopback audio server (see [`super::audio_server`]): WebKitGTK routes
//! `<audio>` through GStreamer, which cannot fetch custom schemes, so bytes
//! are served over HTTP on 127.0.0.1 with ranges. Everything testable (range
//! math, edition resolution, publication descriptors) lives here.
//!
//! EPUBs open in a dedicated `/reader/pub/[editionId]` window backed by an
//! in-memory [`Reader`](livtet_reader::Reader) per edition:
//! [`resolve_reader`] is the shared fail-closed EPUB entry point — it parses
//! the edition id, resolves the catalog file exactly like `get_edition_detail`,
//! rejects missing or non-EPUB files, and opens (or reuses) the cached reader.
//! The `reader` protocol handler in `crate::run` serves only already-cached
//! publications, so a window can never observe a half-opened book.
//!
//! v1 serves decoded EPUB text through [`reader_resource`] for programmatic
//! use by the navigator; binary resources (images, fonts, audio) are served
//! over the `reader` protocol instead of IPC.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use livtet_core::data::entities::{digital_inventory, editions};
use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
use livtet_types::{DbId, KnownFormats};

use crate::error::EpubError;
use crate::types::AppState;

/// Reader backend errors.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReaderError {
    pub(crate) code: String,
    pub(crate) message: String,
}

impl ReaderError {
    pub(crate) fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl From<livtet_core::data::orm::DbErr> for ReaderError {
    fn from(error: livtet_core::data::orm::DbErr) -> Self {
        Self::new("database", error)
    }
}

impl From<EpubError> for ReaderError {
    fn from(error: EpubError) -> Self {
        let code = match &error {
            EpubError::UnknownEdition { .. } => "unknown-edition",
            EpubError::NoFile => "no-file",
            EpubError::MissingFile => "missing-file",
            EpubError::UnsupportedFormat { .. } => "unsupported-format",
            EpubError::Publication { .. } => "publication",
        };
        Self::new(code, error)
    }
}

/// One audiobook chapter for the player UI.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReaderChapter {
    pub name: String,
    pub audio_start: i32,
    pub audio_end: i32,
}

/// The reader descriptor for one edition.
///
/// Audiobooks carry chapter and loopback-URL metadata for the player UI;
/// EPUBs carry the navigator payload. The manifest and positions travel as
/// JSON strings: `serde_json::Value` implements Specta `Type`, but its
/// `Number` arm forbids TypeScript export (BigInt precision), so the
/// structured form cannot leave Rust.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind")]
pub enum ReaderPublication {
    Audiobook {
        edition_id: String,
        title: Option<String>,
        duration_seconds: i32,
        chapters: Vec<ReaderChapter>,
        audio_url: String,
    },
    Epub {
        edition_id: String,
        title: Option<String>,
        base_url: String,
        manifest: String,
        positions: String,
    },
}

/// Label prefix for reader windows; the edition id follows the prefix.
pub const READER_WINDOW_PREFIX: &str = "reader-";

/// Inclusive `[start, end]` byte range for a `Range` header value.
///
/// `None` means "serve 200 with the whole file": missing, multi-range, or
/// unsatisfiable ranges.
pub(crate) fn parse_range_header(total_len: u64, header: &str) -> Option<(u64, u64)> {
    if total_len == 0 {
        return None;
    }
    let spec = header.strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let last = total_len - 1;
    if start.trim().is_empty() {
        // Suffix range: the last N bytes.
        let suffix: u64 = end.trim().parse().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some((total_len.saturating_sub(suffix), last));
    }
    let start: u64 = start.trim().parse().ok()?;
    if start > last {
        return None;
    }
    let end: u64 = if end.trim().is_empty() {
        last
    } else {
        end.trim().parse().ok()?
    };
    if end < start {
        return None;
    }
    Some((start, end.min(last)))
}

/// Resolve the on-disk audio file for an audiobook edition.
///
/// Fails closed when the edition is missing, is not an audiobook, or has no
/// stored file.
pub(crate) async fn resolve_reader_audio(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
) -> Result<camino::Utf8PathBuf, ReaderError> {
    let edition = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    if edition.format_id != Some(KnownFormats::Audiobook.into()) {
        return Err(ReaderError::new(
            "unsupported",
            "the reader serves audiobook editions only",
        ));
    }
    let inventory = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition has no stored file"))?;
    inventory
        .file_path
        .filter(|path| !path.trim().is_empty())
        .map(camino::Utf8PathBuf::from)
        .ok_or_else(|| ReaderError::new("not-found", "edition has no stored file"))
}

/// Describe the audiobook edition for the reader window. `Ok(None)` when
/// absent.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn describe_publication(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    audio: &super::audio_server::AudioServer,
) -> Result<Option<ReaderPublication>, ReaderError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Ok(None);
    };
    if edition.format_id != Some(KnownFormats::Audiobook.into()) {
        return Err(ReaderError::new(
            "unsupported",
            "the reader serves audiobook editions only",
        ));
    }
    let format = edition.format_metadata.unwrap_or(serde_json::Value::Null);
    let duration_seconds = format
        .get("duration_seconds")
        .and_then(serde_json::Value::as_i64)
        .and_then(|duration| i32::try_from(duration).ok())
        .ok_or_else(|| ReaderError::new("invalid", "audiobook is missing its duration"))?;
    let mut chapters = Vec::new();
    for chapter in format
        .get("chapters")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        chapters.push(ReaderChapter {
            name: chapter
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            audio_start: chapter
                .get("audio_start")
                .and_then(serde_json::Value::as_i64)
                .and_then(|start| i32::try_from(start).ok())
                .unwrap_or(0),
            audio_end: chapter
                .get("audio_end")
                .and_then(serde_json::Value::as_i64)
                .and_then(|end| i32::try_from(end).ok())
                .unwrap_or(0),
        });
    }
    Ok(Some(ReaderPublication::Audiobook {
        edition_id: edition_id.to_string(),
        title: edition.title,
        duration_seconds,
        chapters,
        audio_url: format!("{}/audio/{edition_id}?t={}", audio.base_url, audio.token),
    }))
}

/// Whether `file_path` names an entry of the library store.
///
/// Lexical check only: the path must be absolute and, after resolving `.`
/// and `..` without touching the filesystem, stay under `books_dir`.
/// Symlinks are deliberately NOT resolved — link-mode imports are symlinks
/// inside `books_dir` pointing elsewhere, and that is the design.
/// Relative paths never name a library entry.
pub(crate) fn reader_path_in_library(
    books_dir: &camino::Utf8Path,
    file_path: &camino::Utf8Path,
) -> bool {
    use std::path::Component;
    let mut normalized = std::path::PathBuf::new();
    for component in file_path.as_std_path().components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return false;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if !normalized.is_absolute() {
        return false;
    }
    let mut base = std::path::PathBuf::new();
    for component in books_dir.as_std_path().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !base.pop() {
                    return false;
                }
            }
            _ => base.push(component.as_os_str()),
        }
    }
    normalized.starts_with(&base)
}

/// Read the inclusive `[start, end]` byte window of an open file.
pub(crate) async fn read_byte_range(
    mut file: tokio::fs::File,
    start: u64,
    end: u64,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let len = end.saturating_sub(start).saturating_add(1);
    // A whole-file read of a large audiobook is bounded only by the file
    // itself; media elements always range, so this path stays exceptional.
    let mut body = vec![
        0u8;
        usize::try_from(len).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "range too large")
        })?
    ];
    file.seek(std::io::SeekFrom::Start(start)).await?;
    file.read_exact(&mut body).await?;
    Ok(body)
}

/// An opened (or reused) EPUB publication plus the edition metadata the
/// window needs. Kept free of Tauri state where possible so units can stay
/// focused.
pub(crate) struct ResolvedReader {
    pub id: DbId,
    pub title: Option<String>,
    pub reader: Arc<livtet_reader::Reader>,
}

/// Parse an edition id, rejecting garbage before any query or disk access.
pub(crate) fn parse_edition_id(edition_id: &str) -> Result<DbId, EpubError> {
    edition_id
        .parse::<DbId>()
        .map_err(|_| EpubError::unknown_edition(edition_id))
}

/// Validate the catalog file path for reader use: it must exist on disk and
/// carry an `.epub` extension (compared case-insensitively).
pub(crate) fn check_reader_file(file_path: Option<&str>) -> Result<PathBuf, EpubError> {
    let Some(path) = file_path else {
        return Err(EpubError::NoFile);
    };
    let fs_path = PathBuf::from(path);
    if !fs_path.exists() {
        return Err(EpubError::MissingFile);
    }
    let is_epub = fs_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("epub"));
    if !is_epub {
        let format = fs_path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("unknown");
        return Err(EpubError::unsupported_format(format));
    }
    Ok(fs_path)
}

/// SPA path for an edition's EPUB reader window, without a leading slash.
///
/// The desktop reader window targets this nested route (e.g.
/// `reader/pub/{edition id}`); SvelteKit serves it in SPA fallback mode.
pub(crate) fn reader_pub_window_path(edition_id: &DbId) -> String {
    format!("reader/pub/{edition_id}")
}

/// Base URL for an edition's publication resources. It always ends in `/`.
///
/// Desktop WebViews resolve the `reader` custom scheme as
/// `reader://localhost/`, but Windows and Android map custom schemes onto
/// `http://<scheme>.localhost/` instead, hence the platform split.
pub(crate) fn reader_base_url(edition_id: &DbId) -> String {
    #[cfg(any(windows, target_os = "android"))]
    {
        format!("http://reader.localhost/{edition_id}/")
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        format!("reader://localhost/{edition_id}/")
    }
}

/// Split a `reader` protocol path into `(edition id, archive href)`.
///
/// Expected shape: `/{editionId}/{archive href...}`. The href is returned
/// exactly as received: percent-decoding and validation happen in
/// [`Reader::read`](livtet_reader::Reader::read).
pub(crate) fn parse_reader_uri(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix('/')?;
    let (edition_id, href) = rest.split_once('/')?;
    if edition_id.is_empty() || href.is_empty() {
        return None;
    }
    Some((edition_id, href))
}

/// Look up an already-cached EPUB publication without opening anything.
pub(crate) fn cached_lookup(state: &AppState, id: &DbId) -> Option<Arc<livtet_reader::Reader>> {
    state
        .readers
        .lock()
        .ok()
        .and_then(|cache| cache.get(id).cloned())
}

/// Resolve, open, and cache the EPUB publication for an edition. Cached
/// readers are reused; every failure mode is fail-closed.
pub(crate) async fn resolve_reader(
    state: &AppState,
    edition_id: &str,
) -> Result<ResolvedReader, EpubError> {
    let id = parse_edition_id(edition_id)?;
    if let Some(reader) = cached_lookup(state, &id) {
        let title = super::catalog::fetch_edition_detail(&state.db.db_conn(), id)
            .await
            .map_err(EpubError::publication)?
            .and_then(|detail| detail.title);
        return Ok(ResolvedReader { id, title, reader });
    }

    let detail = super::catalog::fetch_edition_detail(&state.db.db_conn(), id)
        .await
        .map_err(EpubError::publication)?
        .ok_or_else(|| EpubError::unknown_edition(edition_id))?;
    let path = check_reader_file(
        detail
            .file
            .as_ref()
            .and_then(|file| file.file_path.as_deref()),
    )?;
    let reader = Arc::new(livtet_reader::Reader::open(&path)?);
    state
        .readers
        .lock()
        .map_err(|_| EpubError::publication("reader cache unavailable"))?
        .insert(id, Arc::clone(&reader));
    Ok(ResolvedReader {
        id,
        title: detail.title,
        reader,
    })
}

/// Assemble the EPUB navigator payload for a cached publication.
pub(crate) fn assemble_epub_publication(
    id: &DbId,
    title: Option<String>,
    reader: &livtet_reader::Reader,
) -> ReaderPublication {
    let base_url = reader_base_url(id);
    // `Value` serialization is infallible; `null` is a fail-closed fallback.
    let manifest =
        serde_json::to_string(&reader.manifest(&base_url)).unwrap_or_else(|_| "null".to_string());
    let positions =
        serde_json::to_string(&reader.positions()).unwrap_or_else(|_| "null".to_string());
    ReaderPublication::Epub {
        edition_id: id.to_string(),
        title,
        base_url,
        manifest,
        positions,
    }
}

/// Decode one EPUB publication resource as UTF-8 text.
pub(crate) fn read_resource_text(
    reader: &livtet_reader::Reader,
    href: &str,
) -> Result<String, EpubError> {
    let Some((_, bytes)) = reader.read(href) else {
        return Err(EpubError::publication(format!(
            "resource not found: {href}"
        )));
    };
    String::from_utf8(bytes)
        .map_err(|_| EpubError::publication(format!("resource is not UTF-8 text: {href}")))
}

/// Open (or focus) the dedicated audiobook reader window for an edition.
///
/// Fail-closed before touching windows: only describable audiobook editions
/// open.
async fn open_audiobook_reader(
    app: &AppHandle,
    state: &State<'_, AppState>,
    id: DbId,
) -> Result<(), ReaderError> {
    // Fail closed before touching windows: only describable editions open.
    let title = describe_publication(&state.db.db_conn(), id, &state.audio)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    let title = match &title {
        ReaderPublication::Audiobook { title, .. } => {
            title.clone().unwrap_or_else(|| "Reader".to_string())
        }
        ReaderPublication::Epub { .. } => {
            unreachable!("audiobook editions never describe as EPUB")
        }
    };

    let label = format!("{READER_WINDOW_PREFIX}{id}");
    if let Some(window) = app.get_webview_window(&label) {
        window
            .set_focus()
            .map_err(|error| ReaderError::new("window", error))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(format!("reader/{id}").into()))
        .title(title)
        .inner_size(480.0, 800.0)
        .build()
        .map_err(|error| ReaderError::new("window", error))?;
    Ok(())
}

/// Open the EPUB reader window for an edition, focusing it when already open.
///
/// The publication is resolved and cached first so a broken book fails
/// before any window is opened or focused.
async fn open_epub_reader(
    app: &AppHandle,
    state: &State<'_, AppState>,
    edition_id: &str,
) -> Result<(), ReaderError> {
    let resolved = resolve_reader(state, edition_id)
        .await
        .map_err(ReaderError::from)?;
    let label = format!("{READER_WINDOW_PREFIX}{}", resolved.id);
    if let Some(window) = app.get_webview_window(&label) {
        window
            .set_focus()
            .map_err(EpubError::publication)
            .map_err(ReaderError::from)?;
        return Ok(());
    }
    let title = resolved
        .title
        .clone()
        .unwrap_or_else(|| format!("Reader — {}", resolved.id));
    tauri::WebviewWindowBuilder::new(
        app,
        &label,
        tauri::WebviewUrl::App(reader_pub_window_path(&resolved.id).into()),
    )
    .title(title)
    .inner_size(1000.0, 720.0)
    .min_inner_size(480.0, 480.0)
    .build()
    .map_err(EpubError::publication)
    .map_err(ReaderError::from)?;
    Ok(())
}

/// Open (or focus) the dedicated reader window for an edition.
///
/// Audiobook editions target the `reader/{id}` player window; all other
/// editions go through the fail-closed EPUB resolver and target
/// `reader/pub/{id}`.
#[tauri::command]
#[specta::specta]
pub async fn open_reader(
    app: AppHandle,
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<(), ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", format!("invalid edition id: {edition_id}")))?;
    let edition = editions::Entity::find_by_id(id)
        .one(&state.db.db_conn())
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    if edition.format_id == Some(KnownFormats::Audiobook.into()) {
        return open_audiobook_reader(&app, &state, id).await;
    }
    open_epub_reader(&app, &state, &edition_id).await
}

/// Fetch one edition's reader descriptor. `Ok(None)` when no edition with
/// this id exists.
///
/// Audiobooks describe chapters and the loopback audio URL; EPUBs describe
/// the Readium manifest and positions list.
#[tauri::command]
#[specta::specta]
pub async fn reader_publication(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ReaderPublication>, ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", format!("invalid edition id: {edition_id}")))?;
    let db = state.db.db_conn();
    let Some(edition) = editions::Entity::find_by_id(id).one(&db).await? else {
        return Ok(None);
    };
    if edition.format_id == Some(KnownFormats::Audiobook.into()) {
        return describe_publication(&db, id, &state.audio).await;
    }
    let resolved = resolve_reader(&state, &edition_id)
        .await
        .map_err(ReaderError::from)?;
    Ok(Some(assemble_epub_publication(
        &resolved.id,
        resolved.title.clone(),
        &resolved.reader,
    )))
}

/// Read one EPUB publication resource as UTF-8 text.
///
/// v1 is intended only for text resources read programmatically by the
/// navigator; binary resources are served over the `reader` protocol.
#[tauri::command]
#[specta::specta]
pub async fn reader_resource(
    edition_id: String,
    href: String,
    state: State<'_, AppState>,
) -> Result<String, ReaderError> {
    let resolved = resolve_reader(&state, &edition_id)
        .await
        .map_err(ReaderError::from)?;
    read_resource_text(&resolved.reader, &href).map_err(ReaderError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::{Kind, TestDb};

    #[test]
    fn library_fence_accepts_linked_and_copied_entries() {
        use camino::Utf8Path;
        let books = Utf8Path::new("/data/books");
        // A symlink inside books_dir pointing elsewhere is the link-mode
        // design: the check is lexical and never resolves the target.
        assert!(reader_path_in_library(
            books,
            Utf8Path::new("/data/books/ab12-imaan.m4b")
        ));
        assert!(reader_path_in_library(
            books,
            Utf8Path::new("/data/books/./ab12-imaan.m4b")
        ));
    }

    #[test]
    fn library_fence_rejects_escapes() {
        use camino::Utf8Path;
        let books = Utf8Path::new("/data/books");
        assert!(!reader_path_in_library(books, Utf8Path::new("/etc/passwd")));
        assert!(!reader_path_in_library(
            books,
            Utf8Path::new("/data/books/../covers/x.jpg")
        ));
        assert!(!reader_path_in_library(
            books,
            Utf8Path::new("/data/books/a/../../etc/passwd")
        ));
        // Sibling prefix, not containment.
        assert!(!reader_path_in_library(
            books,
            Utf8Path::new("/data/books2/a.m4b")
        ));
        // Relative paths never name a library entry.
        assert!(!reader_path_in_library(books, Utf8Path::new("books/a.m4b")));
    }

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(parse_range_header(1000, "bytes=0-499"), Some((0, 499)));
        assert_eq!(parse_range_header(1000, "bytes=500-"), Some((500, 999)));
        assert_eq!(parse_range_header(1000, "bytes=-200"), Some((800, 999)));
        assert_eq!(parse_range_header(1000, "bytes=0-"), Some((0, 999)));
        // Clamped, unsatisfiable, multi-range, and garbage fall back to 200.
        assert_eq!(parse_range_header(1000, "bytes=0-9999"), Some((0, 999)));
        assert_eq!(parse_range_header(1000, "bytes=999-"), Some((999, 999)));
        assert_eq!(parse_range_header(1000, "bytes=1000-"), None);
        assert_eq!(parse_range_header(1000, "bytes=500-100"), None);
        assert_eq!(parse_range_header(1000, "bytes=0-1, 500-600"), None);
        assert_eq!(parse_range_header(1000, "items=0-10"), None);
        assert_eq!(parse_range_header(0, "bytes=0-"), None);
    }

    struct Fixture {
        edition_id: DbId,
        path: camino::Utf8PathBuf,
        // Kept alive for the test's duration so the fixture file survives.
        _dir: tempfile::TempDir,
    }

    async fn audiobook_fixture(db: &livtet_core::data::orm::DatabaseConnection) -> Fixture {
        use livtet_core::data::entities::works;
        use livtet_core::data::orm::{ActiveModelTrait, Set};
        use livtet_types::now_primitive;

        let now = now_primitive();
        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("Work".to_string()),
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
            title: Set(Some("Book".to_string())),
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

        let dir = tempfile::tempdir().expect("temp dir");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("book.m4b")).expect("utf8 path");
        std::fs::write(&path, b"fake audio").expect("fixture file");

        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(Some(path.to_string())),
            cover_path: Set(None),
            blurhash: Set(None),
            dominant_color: Set(None),
            file_hash: Set(None),
            file_size_bytes: Set(None),
            file_format: Set(Some("m4b".to_string())),
            notes: Set(None),
            added_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("inventory");

        Fixture {
            edition_id,
            path,
            _dir: dir,
        }
    }

    async fn mark_audiobook(
        db: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
        format_metadata: Option<serde_json::Value>,
    ) {
        use livtet_core::data::orm::{ActiveModelTrait, IntoActiveModel, Set};
        let mut active = editions::Entity::find_by_id(edition_id)
            .one(db)
            .await
            .expect("query ok")
            .expect("edition present")
            .into_active_model();
        active.format_id = Set(Some(KnownFormats::Audiobook.into()));
        active.format_metadata = Set(format_metadata);
        active.update(db).await.expect("update ok");
    }

    #[tokio::test]
    async fn resolves_audiobook_files() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(&db, fixture.edition_id, None).await;

        let resolved = resolve_reader_audio(&db, fixture.edition_id)
            .await
            .expect("audiobook resolves");
        assert_eq!(resolved, fixture.path);
    }

    #[tokio::test]
    async fn rejects_non_audiobook_editions() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;

        resolve_reader_audio(&db, fixture.edition_id)
            .await
            .expect_err("an edition without the audiobook format fails closed");
        resolve_reader_audio(&db, DbId::new())
            .await
            .expect_err("a missing edition fails closed");
    }

    #[tokio::test]
    async fn describes_audiobook_publications() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(
            &db,
            fixture.edition_id,
            Some(serde_json::json!({
                "duration_seconds": 7200,
                "chapters": [{"name": "Intro", "audio_start": 0, "audio_end": 300}],
            })),
        )
        .await;

        let audio = super::super::audio_server::AudioServer {
            base_url: "http://127.0.0.1:9".to_string(),
            token: "t".to_string(),
        };
        let publication = describe_publication(&db, fixture.edition_id, &audio)
            .await
            .expect("query ok")
            .expect("publication present");
        assert_eq!(
            publication,
            ReaderPublication::Audiobook {
                edition_id: fixture.edition_id.to_string(),
                title: Some("Book".to_string()),
                duration_seconds: 7200,
                chapters: vec![ReaderChapter {
                    name: "Intro".to_string(),
                    audio_start: 0,
                    audio_end: 300,
                }],
                audio_url: format!("http://127.0.0.1:9/audio/{}?t=t", fixture.edition_id),
            }
        );
    }

    #[tokio::test]
    async fn missing_edition_has_no_publication() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let audio = super::super::audio_server::AudioServer {
            base_url: "http://127.0.0.1:9".to_string(),
            token: "t".to_string(),
        };
        let publication = describe_publication(&db, DbId::new(), &audio)
            .await
            .expect("query ok");
        assert!(publication.is_none());
    }

    const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Reader Command Test</dc:title>
    <dc:creator>Jane Doe</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="ch1" href="ch01.xhtml" media-type="application/xhtml+xml"/>
    <item id="cover" href="cover.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

    /// Minimal in-memory EPUB fixture: OPF plus one chapter and one binary.
    fn fixture() -> tempfile::NamedTempFile {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut cursor);
            let options =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            zip.start_file("mimetype", options).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();
            zip.start_file("META-INF/container.xml", options).unwrap();
            zip.write_all(CONTAINER.as_bytes()).unwrap();
            zip.start_file("OEBPS/content.opf", options).unwrap();
            zip.write_all(OPF.as_bytes()).unwrap();
            zip.start_file("OEBPS/ch01.xhtml", options).unwrap();
            zip.write_all(b"<html><body>One</body></html>").unwrap();
            zip.start_file("OEBPS/cover.jpg", options).unwrap();
            zip.write_all(b"\xff\xd8\xff\xfefake-jpeg").unwrap();
            zip.finish().unwrap();
        }
        let mut file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
        file.write_all(&cursor.into_inner()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn epub_errors_map_to_reader_codes() {
        let err = ReaderError::from(EpubError::unknown_edition("abc"));
        assert_eq!(err.code, "unknown-edition");
        assert!(
            err.message.contains("abc"),
            "unexpected message: {}",
            err.message
        );

        let err = ReaderError::from(EpubError::NoFile);
        assert_eq!(err.code, "no-file");

        let err = ReaderError::from(EpubError::MissingFile);
        assert_eq!(err.code, "missing-file");

        let err = ReaderError::from(EpubError::unsupported_format("pdf"));
        assert_eq!(err.code, "unsupported-format");
        assert!(
            err.message.contains("pdf"),
            "unexpected message: {}",
            err.message
        );

        let err = ReaderError::from(EpubError::publication("boom"));
        assert_eq!(err.code, "publication");
        assert_eq!(err.message, "Publication error: boom");
    }

    #[test]
    fn garbage_edition_id_is_rejected_before_any_query() {
        let err = parse_edition_id("not-a-ulid").expect_err("garbage must fail");
        match err {
            EpubError::UnknownEdition { id } => assert_eq!(id, "not-a-ulid"),
            other => panic!("expected UnknownEdition, got {other:?}"),
        }
    }

    #[test]
    fn valid_edition_id_parses() {
        let id = DbId::new();
        assert_eq!(parse_edition_id(&id.to_string()).expect("valid id"), id);
    }

    #[test]
    fn edition_without_a_file_path_is_rejected() {
        assert!(
            matches!(check_reader_file(None), Err(EpubError::NoFile)),
            "missing file path must be NoFile"
        );
    }

    #[test]
    fn unreachable_file_path_is_rejected() {
        let missing = std::env::temp_dir().join("livtet-reader-missing-book.epub");
        assert!(
            !missing.exists(),
            "test precondition: fixture path must not exist"
        );
        assert!(
            matches!(
                check_reader_file(Some(&missing.to_string_lossy())),
                Err(EpubError::MissingFile)
            ),
            "unreachable file path must be MissingFile"
        );
    }

    #[test]
    fn non_epub_extension_is_rejected_case_insensitively() {
        let dir = tempfile::tempdir().expect("temp dir");
        let pdf = dir.path().join("book.pdf");
        std::fs::write(&pdf, b"%PDF").expect("write pdf stand-in");
        match check_reader_file(Some(&pdf.to_string_lossy())) {
            Err(EpubError::UnsupportedFormat { format }) => assert_eq!(format, "pdf"),
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }

        let upper = dir.path().join("book.EPUB");
        std::fs::write(&upper, b"epub stand-in").expect("write epub stand-in");
        assert!(
            check_reader_file(Some(&upper.to_string_lossy())).is_ok(),
            ".EPUB must be accepted case-insensitively"
        );
    }

    #[test]
    fn missing_resource_returns_a_publication_error() {
        let file = fixture();
        let reader = livtet_reader::Reader::open(file.path()).expect("fixture opens");
        let err = read_resource_text(&reader, "OEBPS/missing.xhtml").expect_err("missing href");
        assert!(
            matches!(err, EpubError::Publication { .. }),
            "missing resource must be a Publication error, got {err:?}"
        );
    }

    #[test]
    fn resource_text_decodes_utf8() {
        let file = fixture();
        let reader = livtet_reader::Reader::open(file.path()).expect("fixture opens");
        let text = read_resource_text(&reader, "OEBPS/ch01.xhtml").expect("chapter reads");
        assert!(text.contains("One"), "unexpected chapter text: {text}");
    }

    #[test]
    fn non_utf8_resource_returns_a_publication_error() {
        let file = fixture();
        let reader = livtet_reader::Reader::open(file.path()).expect("fixture opens");
        let err = read_resource_text(&reader, "OEBPS/cover.jpg").expect_err("binary href");
        assert!(
            matches!(err, EpubError::Publication { .. }),
            "binary resource must be a Publication error, got {err:?}"
        );
    }

    #[test]
    fn reader_pub_window_path_targets_the_nested_spa_route() {
        let id = DbId::new();
        let path = reader_pub_window_path(&id);
        let canonical = id.to_string();
        assert!(
            path.starts_with("reader/pub/"),
            "window path must target the nested route, got {path}"
        );
        assert!(
            !path.starts_with('/'),
            "window path must be relative, got {path}"
        );
        assert!(
            path.ends_with(canonical.as_str()),
            "window path must end with the canonical edition id, got {path}"
        );
    }

    #[test]
    fn publication_base_url_ends_in_slash_for_the_active_platform() {
        let id = DbId::new();
        let base_url = reader_base_url(&id);
        assert!(
            base_url.ends_with('/'),
            "base URL must end in `/`, got {base_url}"
        );
        assert!(
            base_url.contains(&id.to_string()),
            "base URL must carry the edition id, got {base_url}"
        );
        #[cfg(any(windows, target_os = "android"))]
        assert!(
            base_url.starts_with("http://reader.localhost/"),
            "unexpected Windows/Android base URL: {base_url}"
        );
        #[cfg(not(any(windows, target_os = "android")))]
        assert!(
            base_url.starts_with("reader://localhost/"),
            "unexpected desktop base URL: {base_url}"
        );
    }

    #[test]
    fn assembled_epub_publication_links_back_to_its_base_url() {
        let file = fixture();
        let reader = livtet_reader::Reader::open(file.path()).expect("fixture opens");
        let id = DbId::new();
        let publication = assemble_epub_publication(&id, Some("Test Book".to_string()), &reader);
        let ReaderPublication::Epub {
            edition_id,
            title,
            base_url,
            manifest,
            positions,
        } = publication
        else {
            panic!("assembled publication must be the Epub variant");
        };
        assert_eq!(edition_id, id.to_string());
        assert_eq!(title, Some("Test Book".to_string()));
        assert_eq!(base_url, reader_base_url(&id));
        let manifest: serde_json::Value =
            serde_json::from_str(&manifest).expect("manifest is JSON");
        assert_eq!(
            manifest["links"][0]["href"],
            serde_json::json!(format!("{base_url}manifest.json"))
        );
        let positions: serde_json::Value =
            serde_json::from_str(&positions).expect("positions is JSON");
        assert!(positions.is_array(), "positions must serialize to an array");
    }

    #[test]
    fn protocol_paths_split_into_edition_and_href() {
        assert_eq!(
            parse_reader_uri("/abc/OEBPS/ch01.xhtml"),
            Some(("abc", "OEBPS/ch01.xhtml"))
        );
        assert_eq!(
            parse_reader_uri("/abc/a/b/c.css"),
            Some(("abc", "a/b/c.css"))
        );
        assert_eq!(parse_reader_uri("/"), None);
        assert_eq!(parse_reader_uri("/only-edition"), None);
        assert_eq!(parse_reader_uri("/abc/"), None);
        assert_eq!(parse_reader_uri(""), None);
        assert_eq!(parse_reader_uri("no-leading-slash"), None);
    }

    #[test]
    fn reader_errors_map_into_epub_errors() {
        let io = livtet_reader::ReaderError::Package("bad opf".to_string());
        assert!(
            matches!(EpubError::from(io), EpubError::Publication { .. }),
            "livtet-reader failures must surface as Publication errors"
        );
    }
}
