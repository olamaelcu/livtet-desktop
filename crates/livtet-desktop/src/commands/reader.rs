//! Reader window commands: the audiobook descriptor backend and the EPUB
//! streamer/navigator backend behind one dispatching entry point.
//!
//! Audiobooks play in a dedicated `/reader/audio/[editionId]` window backed by the
//! loopback audio server (see [`super::byte_server`]): WebKitGTK routes
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

use camino::Utf8PathBuf;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use livtet_core::data::entities::{
    digital_inventory, editions, reading_bookmarks, reading_progress,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, Set,
};
use livtet_types::now_primitive;
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

/// Map EPUB failures into the shared `{ code, message }` shape.
///
/// The audio backend owns the canonical codes (`database`, `not-found`,
/// `unsupported`, `invalid`, `invalid-id`, `window`); EPUB conditions map
/// onto a parallel vocabulary so audio clients never mistake one for the
/// other. Messages mirror the `EpubError` display strings, and the
/// serialization stays the shared `ReaderError` struct.
///
/// | EPUB condition                           | code                |
/// |------------------------------------------|---------------------|
/// | edition id unparseable or edition absent | `unknown-edition`   |
/// | edition has no catalog file path         | `no-file`           |
/// | path recorded but missing from disk      | `missing-file`      |
/// | file present but not an `.epub`          | `unsupported-format`|
/// | anything else (lock, open, read, window) | `publication`       |
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
    /// A PDF served over the loopback byte server for pdf.js (ADR 0037).
    ///
    /// No page count travels here: pdf.js reports `numPages` once the document
    /// loads, and a second copy could disagree with the file.
    Pdf {
        edition_id: String,
        title: Option<String>,
        pdf_url: String,
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

/// Resolve the on-disk file of an edition the loopback server may serve.
///
/// Fails closed when the edition is missing, carries a different format, or
/// has no stored file. `format_label` names the accepted format in the
/// `unsupported` message, which reaches the user.
pub(crate) async fn resolve_reader_file(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    format: KnownFormats,
    format_label: &str,
) -> Result<camino::Utf8PathBuf, ReaderError> {
    let edition = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    if edition.format_id != Some(format.into()) {
        return Err(ReaderError::new(
            "unsupported",
            format!("the reader serves {format_label} editions only"),
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

/// The format label used for PDF editions in user-facing reader errors.
pub(crate) const PDF_FORMAT_LABEL: &str = "PDF";

/// Describe a PDF edition for its reader window (ADR 0037).
///
/// Fails closed on a missing edition, a non-PDF format, or a missing file, so
/// the window is only ever opened for a book the server can actually serve.
pub(crate) async fn describe_pdf_publication(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    loopback: &super::byte_server::LoopbackServer,
) -> Result<ReaderPublication, ReaderError> {
    let path = resolve_reader_file(db, edition_id, KnownFormats::Pdf, PDF_FORMAT_LABEL).await?;
    check_reader_file(Some(path.as_str()), PDF_EXTENSION).map_err(ReaderError::from)?;
    let title = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await?
        .and_then(|edition| edition.title);
    Ok(ReaderPublication::Pdf {
        edition_id: edition_id.to_string(),
        title,
        pdf_url: format!(
            "{}/pdf/{edition_id}?t={}",
            loopback.base_url, loopback.token
        ),
    })
}

/// Describe the audiobook edition for the reader window. `Ok(None)` when
/// absent.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn describe_publication(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    loopback: &super::byte_server::LoopbackServer,
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
        audio_url: format!(
            "{}/audio/{edition_id}?t={}",
            loopback.base_url, loopback.token
        ),
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
    // Inputs are already `&Utf8Path`, so every component is valid UTF-8 and
    // the lossy conversions below are lossless.
    let mut normalized = Utf8PathBuf::new();
    for component in file_path.as_std_path().components() {
        match component {
            Component::Prefix(prefix) => {
                normalized.push(prefix.as_os_str().to_string_lossy().as_ref());
            }
            Component::RootDir => {
                normalized.push(component.as_os_str().to_string_lossy().as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return false;
                }
            }
            Component::Normal(part) => {
                normalized.push(part.to_string_lossy().as_ref());
            }
        }
    }
    if !normalized.is_absolute() {
        return false;
    }
    let mut base = Utf8PathBuf::new();
    for component in books_dir.as_std_path().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !base.pop() {
                    return false;
                }
            }
            _ => base.push(component.as_os_str().to_string_lossy().as_ref()),
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

/// File extension the EPUB reader accepts.
pub(crate) const EPUB_EXTENSION: &str = "epub";

/// File extension the PDF reader accepts (ADR 0037).
pub(crate) const PDF_EXTENSION: &str = "pdf";

/// Validate the catalog file path for reader use: it must exist on disk and
/// carry the `expected` extension (compared case-insensitively).
///
/// The gate is symmetric by design — an EPUB is as unopenable in the PDF
/// reader as a PDF is in the EPUB one — so a format mix-up fails here rather
/// than inside a parser.
pub(crate) fn check_reader_file(
    file_path: Option<&str>,
    expected: &str,
) -> Result<Utf8PathBuf, EpubError> {
    let Some(path) = file_path else {
        return Err(EpubError::NoFile);
    };
    let fs_path = Utf8PathBuf::from(path);
    if !fs_path.exists() {
        return Err(EpubError::MissingFile);
    }
    let extension = fs_path.extension();
    if !extension.is_some_and(|extension| extension.eq_ignore_ascii_case(expected)) {
        return Err(EpubError::unsupported_format(
            extension.unwrap_or("unknown"),
        ));
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

/// SPA path for an edition's audiobook reader window, without a leading slash.
///
/// The desktop reader window targets this nested route (e.g.
/// `reader/audio/{edition id}`); SvelteKit serves it in SPA fallback mode.
pub(crate) fn reader_audio_window_path(edition_id: &DbId) -> String {
    format!("reader/audio/{edition_id}")
}

/// SPA path for an edition's PDF reader window, without a leading slash.
///
/// The desktop reader window targets this nested route (e.g.
/// `reader/pdf/{edition id}`); SvelteKit serves it in SPA fallback mode.
pub(crate) fn reader_pdf_window_path(edition_id: &DbId) -> String {
    format!("reader/pdf/{edition_id}")
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

/// The in-memory EPUB publication cache keyed by edition id (see
/// [`AppState::readers`]).
pub(crate) type ReaderCache =
    std::sync::Mutex<std::collections::HashMap<DbId, Arc<livtet_reader::Reader>>>;

/// Look up an already-cached EPUB publication without opening anything.
pub(crate) fn cached_lookup(state: &AppState, id: &DbId) -> Option<Arc<livtet_reader::Reader>> {
    cached_reader(&state.readers, id)
}

/// Look up `id` in the publication cache. A poisoned cache reads as empty.
pub(crate) fn cached_reader(
    readers: &ReaderCache,
    id: &DbId,
) -> Option<Arc<livtet_reader::Reader>> {
    readers.lock().ok().and_then(|cache| cache.get(id).cloned())
}

/// Drop `reader` from the cache, but only if it is still the entry for `id`:
/// a concurrent replacement is left alone.
pub(crate) fn evict_reader(readers: &ReaderCache, id: &DbId, reader: &Arc<livtet_reader::Reader>) {
    if let Ok(mut cache) = readers.lock()
        && cache
            .get(id)
            .is_some_and(|cached| Arc::ptr_eq(cached, reader))
    {
        cache.remove(id);
    }
}

/// Resolve, open, and cache the EPUB publication for an edition. Cached
/// readers are reused (the title is still read fresh from the catalog);
/// every failure mode is fail-closed.
pub(crate) async fn resolve_reader(
    db: &livtet_core::data::orm::DatabaseConnection,
    readers: &ReaderCache,
    edition_id: &str,
) -> Result<ResolvedReader, EpubError> {
    let id = parse_edition_id(edition_id)?;
    if let Some(reader) = cached_reader(readers, &id) {
        let title = super::catalog::fetch_edition_detail(db, id)
            .await
            .map_err(EpubError::publication)?
            .and_then(|detail| detail.title);
        return Ok(ResolvedReader { id, title, reader });
    }

    let detail = super::catalog::fetch_edition_detail(db, id)
        .await
        .map_err(EpubError::publication)?
        .ok_or_else(|| EpubError::unknown_edition(edition_id))?;
    let path = check_reader_file(
        detail
            .file
            .as_ref()
            .and_then(|file| file.file_path.as_deref()),
        EPUB_EXTENSION,
    )?;
    let reader = Arc::new(livtet_reader::Reader::open(&path)?);
    readers
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

/// Which reader backend serves an edition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReaderKind {
    /// The loopback-backed audiobook player.
    Audiobook,
    /// pdf.js over the loopback byte server (ADR 0037).
    Pdf,
    /// The fail-closed EPUB resolver; every other format lands here.
    Epub,
}

impl ReaderKind {
    fn for_edition(edition: &editions::Model) -> Self {
        if edition.format_id == Some(KnownFormats::Audiobook.into()) {
            Self::Audiobook
        } else if edition.format_id == Some(KnownFormats::Pdf.into()) {
            Self::Pdf
        } else {
            Self::Epub
        }
    }

    /// `(inner size, minimum inner size)` of the reader window.
    fn window_size(self) -> ((f64, f64), (f64, f64)) {
        match self {
            Self::Audiobook => ((800.0, 800.0), (547.0, 600.0)),
            Self::Pdf | Self::Epub => ((800.0, 1000.0), (569.0, 600.0)),
        }
    }

    /// Map a window focus/build failure onto this backend's error vocabulary.
    fn window_error(self, error: impl std::fmt::Display) -> ReaderError {
        match self {
            Self::Audiobook => ReaderError::new("window", error),
            Self::Pdf | Self::Epub => ReaderError::from(EpubError::publication(error)),
        }
    }
}

/// Parse `edition_id` and pick the reader backend for its edition.
/// `Ok(None)` when no edition with this id exists.
pub(crate) async fn reader_dispatch(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: &str,
) -> Result<Option<(DbId, ReaderKind)>, ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", format!("invalid edition id: {edition_id}")))?;
    let Some(edition) = editions::Entity::find_by_id(id).one(db).await? else {
        return Ok(None);
    };
    Ok(Some((id, ReaderKind::for_edition(&edition))))
}

/// Reject a virtual book: an inventory row that exists but carries no file
/// path. A virtual book is catalog metadata only, so there is nothing to open,
/// no position to save, and no page to bookmark. A *missing* inventory row is
/// left to the caller's own `not-found` / `no-file` error so those codes stay
/// unchanged.
pub(crate) async fn ensure_not_virtual(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
) -> Result<(), ReaderError> {
    let inventory = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await?;
    let virtual_book = matches!(
        inventory,
        Some(row) if row.file_path.as_deref().is_none_or(|path| path.trim().is_empty())
    );
    if virtual_book {
        return Err(ReaderError::new(
            "virtual-book",
            "virtual books have no file to read",
        ));
    }
    Ok(())
}

/// The title carried by any publication variant.
fn publication_title(publication: ReaderPublication) -> Option<String> {
    match publication {
        ReaderPublication::Audiobook { title, .. }
        | ReaderPublication::Epub { title, .. }
        | ReaderPublication::Pdf { title, .. } => title,
    }
}

/// Everything needed to open (or focus) one reader window.
pub(crate) struct ReaderWindowPlan {
    pub id: DbId,
    pub kind: ReaderKind,
    pub label: String,
    /// SPA route of the window, without a leading slash.
    pub path: String,
    pub title: String,
    /// The EPUB publication this open cached (or reused); `None` for
    /// audiobooks and PDFs, whose bytes stream over the loopback server.
    pub epub: Option<Arc<livtet_reader::Reader>>,
}

/// Resolve the reader window for an edition before any window is touched,
/// so a missing, mis-formatted, or broken book fails with no window ever
/// appearing. EPUBs are opened and cached here.
pub(crate) async fn plan_reader_window(
    db: &livtet_core::data::orm::DatabaseConnection,
    readers: &ReaderCache,
    loopback: &super::byte_server::LoopbackServer,
    edition_id: &str,
) -> Result<ReaderWindowPlan, ReaderError> {
    let (id, kind) = reader_dispatch(db, edition_id)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    ensure_not_virtual(db, id).await?;
    let label = format!("{READER_WINDOW_PREFIX}{id}");
    let plan = match kind {
        ReaderKind::Audiobook => {
            let publication = describe_publication(db, id, loopback)
                .await?
                .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
            ReaderWindowPlan {
                id,
                kind,
                label,
                path: reader_audio_window_path(&id),
                title: publication_title(publication).unwrap_or_else(|| "Reader".to_string()),
                epub: None,
            }
        }
        ReaderKind::Pdf => {
            let publication = describe_pdf_publication(db, id, loopback).await?;
            ReaderWindowPlan {
                id,
                kind,
                label,
                path: reader_pdf_window_path(&id),
                title: publication_title(publication).unwrap_or_else(|| format!("Reader — {id}")),
                epub: None,
            }
        }
        ReaderKind::Epub => {
            let resolved = resolve_reader(db, readers, edition_id)
                .await
                .map_err(|error| {
                    tracing::warn!(edition_id = %edition_id, error = %error, "failed to resolve EPUB publication");
                    ReaderError::from(error)
                })?;
            ReaderWindowPlan {
                id,
                kind,
                label,
                path: reader_pub_window_path(&id),
                title: resolved.title.unwrap_or_else(|| format!("Reader — {id}")),
                epub: Some(resolved.reader),
            }
        }
    };
    tracing::debug!(edition_id = %id, kind = ?kind, window_label = %plan.label, window_path = %plan.path, "planned reader window");
    Ok(plan)
}

/// Open (or focus) the planned reader window for an edition.
async fn open_reader_window(
    app: &AppHandle,
    state: &AppState,
    edition_id: &str,
) -> Result<(), ReaderError> {
    let ReaderWindowPlan {
        id,
        kind,
        label,
        path,
        title,
        epub,
    } = plan_reader_window(
        &state.db.db_conn(),
        &state.readers,
        &state.loopback,
        edition_id,
    )
    .await?;
    if let Some(window) = app.get_webview_window(&label) {
        tracing::info!(edition_id = %id, kind = ?kind, window_label = %label, "focusing existing reader window");
        return window.set_focus().map_err(|error| kind.window_error(error));
    }
    let ((width, height), (min_width, min_height)) = kind.window_size();
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App(path.clone().into()))
        .title(title)
        .inner_size(width, height)
        .min_inner_size(min_width, min_height)
        .build()
        .map_err(|error| {
            tracing::warn!(edition_id = %id, kind = ?kind, window_label = %label, error = %error, "failed to build reader window");
            // No window will serve this publication: evict the entry this
            // open cached so a later open retries from disk instead of
            // reusing an orphan.
            if let Some(reader) = &epub {
                evict_reader(&state.readers, &id, reader);
            }
            kind.window_error(error)
        })?;
    tracing::info!(edition_id = %id, kind = ?kind, window_label = %label, window_path = %path, "opened reader window");
    Ok(())
}

/// Open (or focus) the dedicated reader window for an edition.
///
/// Audiobook editions target the `reader/audio/{id}` player window, PDFs the
/// `reader/pdf/{id}` window; all other editions go through the fail-closed
/// EPUB resolver and target `reader/pub/{id}`.
#[tauri::command]
#[specta::specta]
pub async fn open_reader(
    app: AppHandle,
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<(), ReaderError> {
    tracing::info!(edition_id = %edition_id, "open_reader requested");
    let result = open_reader_window(&app, &state, &edition_id).await;
    match &result {
        Ok(()) => tracing::info!(edition_id = %edition_id, "open_reader completed"),
        Err(error) => {
            tracing::warn!(edition_id = %edition_id, error = ?error, "open_reader failed")
        }
    }
    result
}

/// Describe one edition for its reader. `Ok(None)` when no edition with
/// this id exists.
pub(crate) async fn publication_for_edition(
    db: &livtet_core::data::orm::DatabaseConnection,
    readers: &ReaderCache,
    loopback: &super::byte_server::LoopbackServer,
    edition_id: &str,
) -> Result<Option<ReaderPublication>, ReaderError> {
    let Some((id, kind)) = reader_dispatch(db, edition_id).await? else {
        return Ok(None);
    };
    ensure_not_virtual(db, id).await?;
    match kind {
        ReaderKind::Audiobook => describe_publication(db, id, loopback).await,
        ReaderKind::Pdf => describe_pdf_publication(db, id, loopback).await.map(Some),
        ReaderKind::Epub => {
            let resolved = resolve_reader(db, readers, edition_id).await?;
            Ok(Some(assemble_epub_publication(
                &resolved.id,
                resolved.title,
                &resolved.reader,
            )))
        }
    }
}

/// Fetch one edition's reader descriptor. `Ok(None)` when no edition with
/// this id exists.
///
/// Audiobooks describe chapters and the loopback audio URL; PDFs the
/// loopback PDF URL; EPUBs describe the Readium manifest and positions list.
#[tauri::command]
#[specta::specta]
pub async fn reader_publication(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ReaderPublication>, ReaderError> {
    publication_for_edition(
        &state.db.db_conn(),
        &state.readers,
        &state.loopback,
        &edition_id,
    )
    .await
}

/// Resolve an edition's EPUB and decode one of its resources as UTF-8 text.
pub(crate) async fn resource_text_for_edition(
    db: &livtet_core::data::orm::DatabaseConnection,
    readers: &ReaderCache,
    edition_id: &str,
    href: &str,
) -> Result<String, ReaderError> {
    let resolved = resolve_reader(db, readers, edition_id).await?;
    Ok(read_resource_text(&resolved.reader, href)?)
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
    resource_text_for_edition(&state.db.db_conn(), &state.readers, &edition_id, &href).await
}

/// Upsert per-edition progress into `reading_progress`.
///
/// One writer serves every reader family that shares the table. `kind` is
/// `epub` (stores `locator`), `pdf` (stores `page_number`), or `audio`
/// (stores `position_seconds`, clamped to the edition's duration). Rows are
/// keyed by `(edition_id, format_id)`.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn store_reader_position(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    kind: &str,
    locator: Option<String>,
    page_number: Option<u32>,
    position_seconds: Option<f64>,
    annotations: Option<String>,
) -> Result<(), ReaderError> {
    let unit = reader_progress_unit(kind)?;
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Err(ReaderError::new("not-found", "edition not found"));
    };
    ensure_not_virtual(db, edition_id).await?;
    let format_id = edition
        .format_id
        .ok_or_else(|| ReaderError::new("invalid-format", "edition has no format_id"))?;

    let (progress, last_location) = match unit {
        "epub" => (0.0, locator),
        "pdf" => (page_number.map_or(0.0, f64::from), None),
        // `timestamp`: audiobook playback seconds, clamped to the duration.
        _ => {
            if edition.format_id != Some(KnownFormats::Audiobook.into()) {
                return Err(ReaderError::new(
                    "unsupported",
                    "listening progress is for audiobook editions only",
                ));
            }
            let raw = position_seconds.ok_or_else(|| {
                ReaderError::new("invalid", "position seconds are required for audio")
            })?;
            if !raw.is_finite() {
                return Err(ReaderError::new("invalid", "position must be finite"));
            }
            let position = match edition_duration_seconds(&edition) {
                Some(total) => raw.clamp(0.0, total),
                None => raw.max(0.0),
            };
            (position, Some(position.to_string()))
        }
    };

    let existing = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(format_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut active = row.into_active_model();
            active.progress = Set(progress);
            active.progress_unit = Set(Some(unit.to_string()));
            active.last_location = Set(last_location);
            // `None` means "this writer does not own annotations": a position
            // save must never blank notes written by another save. Callers that
            // own the list always send one (an empty array clears it).
            if let Some(annotations) = annotations {
                active.annotations = Set(Some(annotations));
            }
            active.update(db).await?;
        }
        None => {
            reading_progress::ActiveModel {
                id: Set(DbId::new()),
                edition_id: Set(edition_id),
                format_id: Set(format_id),
                progress: Set(progress),
                progress_unit: Set(Some(unit.to_string())),
                last_location: Set(last_location),
                total_reading_time_secs: Set(0),
                annotations: Set(annotations),
                created_at: Set(now_primitive()),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// Map a caller-facing kind onto the `reading_progress.progress_unit` value.
/// `audio` persists as `timestamp`, matching the audiobook writer and
/// `fetch_reader_position`.
fn reader_progress_unit(kind: &str) -> Result<&'static str, ReaderError> {
    match kind {
        "epub" => Ok("epub"),
        "pdf" => Ok("pdf"),
        "audio" | "timestamp" => Ok("timestamp"),
        _ => Err(ReaderError::new(
            "invalid-kind",
            "kind must be 'epub', 'pdf' or 'audio'",
        )),
    }
}

/// Total audiobook duration in seconds from the edition's format metadata.
fn edition_duration_seconds(edition: &editions::Model) -> Option<f64> {
    edition
        .format_metadata
        .as_ref()?
        .get("duration_seconds")?
        .as_i64()
        .map(|duration| duration as f64)
}

/// Save a reader position (upsert) using the `reading_progress` table.
#[tauri::command]
#[specta::specta]
pub async fn save_reader_position(
    edition_id: String,
    kind: String,
    locator: Option<String>,
    page_number: Option<u32>,
    position_seconds: Option<f64>,
    annotations: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), ReaderError> {
    let edition_id_db = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", "edition id is invalid"))?;
    store_reader_position(
        &state.db.db_conn(),
        edition_id_db,
        &kind,
        locator,
        page_number,
        position_seconds,
        annotations,
    )
    .await
}

/// Persisted reading position for one edition, read from `reading_progress`.
///
/// Covers all three reader families that share the table: EPUB (locator),
/// PDF (page), and audiobook (playback seconds, `progress_unit = "timestamp"`).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReaderPosition {
    /// Reader family: `"epub"`, `"pdf"` or `"audio"`.
    pub kind: String,
    /// Serialized Readium locator, for EPUB editions.
    pub locator: Option<String>,
    /// Current page, for PDF editions.
    pub page_number: Option<u32>,
    /// Playback position in seconds, for audiobook editions.
    pub position_seconds: Option<f64>,
    /// Serialized JSON array of user annotations.
    pub annotations: Option<String>,
}

/// Read the persisted position for an edition. `Ok(None)` when the edition is
/// unknown, has no row, or carries a `progress_unit` this reader does not
/// recognize.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn fetch_reader_position(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<ReaderPosition>, ReaderError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Ok(None);
    };
    let Some(format_id) = edition.format_id else {
        return Ok(None);
    };
    let Some(row) = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(format_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    match row.progress_unit.as_deref() {
        Some("epub") => Ok(Some(ReaderPosition {
            kind: "epub".to_string(),
            locator: row.last_location,
            annotations: row.annotations,
            page_number: None,
            position_seconds: None,
        })),
        Some("pdf") => Ok(Some(ReaderPosition {
            kind: "pdf".to_string(),
            locator: None,
            annotations: row.annotations,
            page_number: Some(row.progress.max(0.0) as u32),
            position_seconds: None,
        })),
        Some("timestamp") => Ok(Some(ReaderPosition {
            kind: "audio".to_string(),
            locator: None,
            annotations: row.annotations,
            page_number: None,
            position_seconds: Some(row.progress.max(0.0)),
        })),
        _ => Ok(None),
    }
}

/// Fetch the persisted reader position for an edition. `Ok(None)` when none
/// has been stored (or the edition is unknown).
#[tauri::command]
#[specta::specta]
pub async fn get_reader_position(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ReaderPosition>, ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", "edition id is invalid"))?;
    fetch_reader_position(&state.db.db_conn(), id).await
}

/// One persisted page bookmark: a location the reader flagged, carrying no
/// note text and no highlight colour.
///
/// The position is the same shape as [`ReaderPosition`], so a bookmark works
/// for every reader family: an EPUB locator, a PDF or physical page number, or
/// an audiobook playback position.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReaderBookmark {
    /// Row id, as a ULID string; the handle to delete the bookmark with.
    pub id: String,
    /// Reader family the bookmark was taken in: `"epub"`, `"pdf"`, `"page"`
    /// (physical books) or `"timestamp"` (audiobooks).
    pub kind: String,
    /// Serialized Readium locator, for EPUB bookmarks.
    pub locator: Option<String>,
    /// Bookmarked page, for PDF and physical book editions.
    pub page_number: Option<u32>,
    /// Bookmarked playback position in seconds, for audiobook editions.
    pub position_seconds: Option<f64>,
    /// Raw `reading_bookmarks.progress`, read in the unit its kind names: the
    /// fraction of the whole publication for EPUB bookmarks (never the
    /// fraction of the chapter), the page number for page bookmarks, and the
    /// playback seconds for audiobooks.
    pub progress: f64,
    /// Creation time, in seconds since the Unix epoch. `f64` rather than an
    /// integer because Specta refuses to export 64-bit integers to JS.
    pub created_at: f64,
}

/// Map a caller-facing bookmark kind onto the `reading_bookmarks.progress_unit`
/// value. `"pdf"` and `"page"` stay distinct so a physical book's page number
/// is not mistaken for a PDF position.
fn reader_bookmark_unit(kind: &str) -> Result<&'static str, ReaderError> {
    match kind {
        "epub" => Ok("epub"),
        "pdf" => Ok("pdf"),
        "page" => Ok("page"),
        "audio" | "timestamp" => Ok("timestamp"),
        _ => Err(ReaderError::new(
            "invalid-kind",
            "kind must be 'epub', 'pdf', 'page' or 'audio'",
        )),
    }
}

/// Turn a stored row into the reader-facing shape.
fn bookmark_from_row(row: reading_bookmarks::Model) -> ReaderBookmark {
    let kind = row.progress_unit.clone().unwrap_or_default();
    let page = u32::try_from(row.progress.max(0.0) as u64).ok();
    let (locator, page_number, position_seconds) = match kind.as_str() {
        "epub" => (row.last_location, None, None),
        "pdf" | "page" => (None, page, None),
        "timestamp" => (None, None, Some(row.progress.max(0.0))),
        // An unknown unit keeps its opaque locator so nothing is dropped from
        // the reader's view while the UI ignores what it cannot render.
        _ => (row.last_location, None, None),
    };
    ReaderBookmark {
        id: row.id.to_string(),
        kind,
        locator,
        page_number,
        position_seconds,
        progress: row.progress,
        created_at: row.created_at.assume_utc().unix_timestamp() as f64,
    }
}

/// Insert a bookmark for an edition. The stored number follows the kind: an EPUB
/// bookmark keeps its locator and stores the fraction of the publication it
/// names, a page bookmark stores its 1-based page, and an audiobook bookmark
/// stores its playback seconds.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn store_reader_bookmark(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
    kind: &str,
    locator: Option<String>,
    page_number: Option<u32>,
    position_seconds: Option<f64>,
    fraction: Option<f64>,
) -> Result<ReaderBookmark, ReaderError> {
    let unit = reader_bookmark_unit(kind)?;
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Err(ReaderError::new("not-found", "edition not found"));
    };
    ensure_not_virtual(db, edition_id).await?;
    // A page number larger than the edition's own surface is still stored:
    // the bookmark records where the reader was, not where the file ends.
    let progress = match unit {
        // An EPUB bookmark records where the page sits in the whole book: the
        // locator's own `progression` is relative to its resource, so it would
        // read as "40% through" for a page 25% into the book.
        "epub" => {
            let fraction = fraction.ok_or_else(|| {
                ReaderError::new(
                    "invalid",
                    "a fraction of the publication is required for EPUB bookmarks",
                )
            })?;
            if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                return Err(ReaderError::new(
                    "invalid",
                    "the fraction must be between 0 and 1",
                ));
            }
            fraction
        }
        "pdf" | "page" => f64::from(page_number.ok_or_else(|| {
            ReaderError::new("invalid", "a page number is required for page bookmarks")
        })?),
        _ => {
            if edition.format_id != Some(KnownFormats::Audiobook.into()) {
                return Err(ReaderError::new(
                    "unsupported",
                    "listening bookmarks are for audiobook editions only",
                ));
            }
            let raw = position_seconds.ok_or_else(|| {
                ReaderError::new(
                    "invalid",
                    "position seconds are required for audio bookmarks",
                )
            })?;
            if !raw.is_finite() {
                return Err(ReaderError::new("invalid", "position must be finite"));
            }
            raw.max(0.0)
        }
    };
    let last_location = match unit {
        "epub" => Some(locator.ok_or_else(|| {
            ReaderError::new("invalid", "a locator is required for EPUB bookmarks")
        })?),
        "timestamp" => Some(progress.to_string()),
        _ => None,
    };

    let row = reading_bookmarks::ActiveModel {
        id: Set(DbId::new()),
        edition_id: Set(edition_id),
        progress: Set(progress),
        progress_unit: Set(Some(unit.to_string())),
        last_location: Set(last_location),
        created_at: Set(now_primitive()),
    }
    .insert(db)
    .await?;
    Ok(bookmark_from_row(row))
}

/// Delete one bookmark. `false` when the row was already gone, so a repeated
/// click is not an error.
pub(crate) async fn delete_reader_bookmark(
    db: &livtet_core::data::orm::DatabaseConnection,
    bookmark_id: DbId,
) -> Result<bool, ReaderError> {
    let result = reading_bookmarks::Entity::delete_by_id(bookmark_id)
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Every bookmark on an edition, oldest first — the order the reader made them
/// in, so the list reads as a trail through the book.
pub(crate) async fn fetch_reader_bookmarks(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<ReaderBookmark>, ReaderError> {
    let rows = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(edition_id))
        .order_by_asc(reading_bookmarks::Column::CreatedAt)
        .order_by_asc(reading_bookmarks::Column::Id)
        .all(db)
        .await?;
    Ok(rows.into_iter().map(bookmark_from_row).collect())
}

/// Bookmark the reader's current location in an edition.
#[tauri::command]
#[specta::specta]
pub async fn add_reader_bookmark(
    edition_id: String,
    kind: String,
    locator: Option<String>,
    page_number: Option<u32>,
    position_seconds: Option<f64>,
    fraction: Option<f64>,
    state: State<'_, AppState>,
) -> Result<ReaderBookmark, ReaderError> {
    let edition_id_db = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", "edition id is invalid"))?;
    store_reader_bookmark(
        &state.db.db_conn(),
        edition_id_db,
        &kind,
        locator,
        page_number,
        position_seconds,
        fraction,
    )
    .await
}

/// Remove one bookmark by its id.
#[tauri::command]
#[specta::specta]
pub async fn remove_reader_bookmark(
    bookmark_id: String,
    state: State<'_, AppState>,
) -> Result<bool, ReaderError> {
    let id = bookmark_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", "bookmark id is invalid"))?;
    delete_reader_bookmark(&state.db.db_conn(), id).await
}

/// Persisted bookmarks for an edition, oldest first.
#[tauri::command]
#[specta::specta]
pub async fn list_reader_bookmarks(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ReaderBookmark>, ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", "edition id is invalid"))?;
    fetch_reader_bookmarks(&state.db.db_conn(), id).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use livtet_core::data::{Kind, TestDb};

    #[tokio::test]
    async fn reader_position_upserts_locator_and_page() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;
        mark_format(&db, id, KnownFormats::Epub).await;

        store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"x\":1}".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save epub locator");
        let saved = reading_progress::Entity::find()
            .filter(reading_progress::Column::EditionId.eq(id))
            .one(&db)
            .await
            .expect("query")
            .expect("row written");
        assert_eq!(saved.progress_unit.as_deref(), Some("epub"));
        assert_eq!(saved.last_location.as_deref(), Some("{\"x\":1}"));

        // Same edition, saving again updates in place (upsert, not a second row).
        store_reader_position(&db, id, "pdf", None, Some(7), None, None)
            .await
            .expect("save pdf page");
        let saved = reading_progress::Entity::find()
            .filter(reading_progress::Column::EditionId.eq(id))
            .one(&db)
            .await
            .expect("query")
            .expect("row remains");
        assert_eq!(saved.progress, 7.0);
        assert_eq!(saved.progress_unit.as_deref(), Some("pdf"));

        // Read back what the DB persisted.
        let saved_pos = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(saved_pos.kind, "pdf");
        assert_eq!(saved_pos.page_number, Some(7));
        assert_eq!(saved_pos.locator, None);

        let error = store_reader_position(&db, id, "video", None, None, None, None)
            .await
            .expect_err("unknown kind");
        assert_eq!(error.code, "invalid-kind");
    }

    #[tokio::test]
    async fn virtual_books_reject_progress_and_bookmarks() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;
        mark_format(&db, id, KnownFormats::Epub).await;

        // Turn the edition into a virtual book: keep the inventory row, clear
        // its file path.
        let mut inventory: digital_inventory::ActiveModel = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(id))
            .one(&db)
            .await
            .expect("query inventory")
            .expect("inventory row")
            .into();
        inventory.file_path = Set(None);
        inventory.update(&db).await.expect("clear file path");

        let error = store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"x\":1}".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect_err("progress rejected");
        assert_eq!(error.code, "virtual-book");
        assert!(
            reading_progress::Entity::find()
                .filter(reading_progress::Column::EditionId.eq(id))
                .one(&db)
                .await
                .expect("query progress")
                .is_none(),
            "no progress row is written for a virtual book"
        );

        let error = store_reader_bookmark(&db, id, "pdf", None, Some(1), None, None)
            .await
            .expect_err("bookmark rejected");
        assert_eq!(error.code, "virtual-book");
    }

    #[tokio::test]
    async fn reader_position_reads_back_from_the_database() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let epub = epub_fixture(&db).await;
        let id = epub.edition_id;
        mark_format(&db, id, KnownFormats::Epub).await;

        // No row yet.
        assert!(
            fetch_reader_position(&db, id)
                .await
                .expect("read")
                .is_none()
        );

        store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"href\":\"c3\"}".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save epub locator");
        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("epub row");
        assert_eq!(saved.kind, "epub");
        assert_eq!(saved.locator.as_deref(), Some("{\"href\":\"c3\"}"));
        assert_eq!(saved.page_number, None);

        // Save as PDF; the row is replaced in place.
        store_reader_position(&db, id, "pdf", None, Some(12), None, None)
            .await
            .expect("save pdf page");
        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("pdf row");
        assert_eq!(saved.kind, "pdf");
        assert_eq!(saved.page_number, Some(12));

        // Unknown edition reads as None rather than erroring.
        assert!(
            fetch_reader_position(&db, DbId::new())
                .await
                .expect("read")
                .is_none()
        );
    }

    #[tokio::test]
    async fn reader_position_round_trips_annotations() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;
        mark_format(&db, id, KnownFormats::Epub).await;

        let annotations = r#"[{"id":"a1","text":"Key point","locator":"{\"href\":\"c1\"}"}]"#;
        store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"href\":\"c1\"}".to_string()),
            None,
            None,
            Some(annotations.to_string()),
        )
        .await
        .expect("save with annotations");

        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(saved.annotations.as_deref(), Some(annotations));
        assert_eq!(saved.locator.as_deref(), Some("{\"href\":\"c1\"}"));
    }

    #[tokio::test]
    async fn position_saves_never_blank_annotations() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;
        mark_format(&db, id, KnownFormats::Epub).await;

        let annotations = r#"[{"id":"a1","text":"Key point"}]"#;
        store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"href\":\"c1\"}".to_string()),
            None,
            None,
            Some(annotations.to_string()),
        )
        .await
        .expect("save with annotations");

        // A position-only writer (the debounced save on open) must not own the
        // column: without this, opening a book silently deleted every note.
        store_reader_position(
            &db,
            id,
            "epub",
            Some("{\"href\":\"c2\"}".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("save position only");

        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(saved.annotations.as_deref(), Some(annotations));
        assert_eq!(saved.locator.as_deref(), Some("{\"href\":\"c2\"}"));
    }

    #[tokio::test]
    async fn reader_position_reads_back_an_audiobook_position() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;
        mark_format(&db, id, KnownFormats::Audiobook).await;
        let format_id = editions::Entity::find_by_id(id)
            .one(&db)
            .await
            .expect("query")
            .expect("edition")
            .format_id
            .expect("format id");

        // Audiobook rows carry seconds under the `timestamp` unit.
        reading_progress::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(id),
            format_id: Set(format_id),
            progress: Set(37.5),
            progress_unit: Set(Some("timestamp".to_string())),
            last_location: Set(Some("37.5".to_string())),
            total_reading_time_secs: Set(0),
            annotations: Set(None),
            created_at: Set(now_primitive()),
        }
        .insert(&db)
        .await
        .expect("insert audiobook progress");

        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("audio row");
        assert_eq!(saved.kind, "audio");
        assert_eq!(saved.position_seconds, Some(37.5));
        assert_eq!(saved.page_number, None);
        assert_eq!(saved.locator, None);
    }

    #[tokio::test]
    async fn audio_positions_clamp_to_the_duration() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        let id = fixture.edition_id;
        mark_audiobook(
            &db,
            id,
            Some(serde_json::json!({ "duration_seconds": 7200 })),
        )
        .await;

        store_reader_position(&db, id, "audio", None, None, Some(99_999.0), None)
            .await
            .expect("save audio");
        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(saved.kind, "audio");
        assert_eq!(saved.position_seconds, Some(7200.0));

        store_reader_position(&db, id, "audio", None, None, Some(-12.0), None)
            .await
            .expect("save audio");
        let saved = fetch_reader_position(&db, id)
            .await
            .expect("read")
            .expect("row");
        assert_eq!(saved.position_seconds, Some(0.0));
    }

    #[tokio::test]
    async fn audio_position_requires_an_audiobook_edition() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        mark_format(&db, edition.edition_id, KnownFormats::Epub).await;

        let error = store_reader_position(
            &db,
            edition.edition_id,
            "audio",
            None,
            None,
            Some(1.0),
            None,
        )
        .await
        .expect_err("audio on an EPUB is unsupported");
        assert_eq!(error.code, "unsupported");
    }

    /// A fixture root the platform calls absolute: on Windows a bare `/data` is
    /// root-relative (no drive prefix), and `reader_path_in_library` rejects it
    /// before any containment check runs.
    fn fence_root() -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from(if cfg!(windows) { r"C:\data" } else { "/data" })
    }

    #[test]
    fn library_fence_accepts_linked_and_copied_entries() {
        let books = fence_root().join("books");
        // A symlink inside books_dir pointing elsewhere is the link-mode
        // design: the check is lexical and never resolves the target.
        assert!(reader_path_in_library(
            &books,
            &books.join("ab12-imaan.m4b")
        ));
        assert!(reader_path_in_library(
            &books,
            &fence_root().join("books/./ab12-imaan.m4b")
        ));
    }

    #[test]
    fn library_fence_rejects_escapes() {
        use camino::Utf8Path;
        let books = fence_root().join("books");
        assert!(!reader_path_in_library(
            &books,
            &fence_root().join("etc/passwd")
        ));
        assert!(!reader_path_in_library(
            &books,
            &fence_root().join("books/../covers/x.jpg")
        ));
        assert!(!reader_path_in_library(
            &books,
            &fence_root().join("books/a/../../etc/passwd")
        ));
        // Sibling prefix, not containment.
        assert!(!reader_path_in_library(
            &books,
            &fence_root().join("books2/a.m4b")
        ));
        // Relative paths never name a library entry.
        assert!(!reader_path_in_library(
            &books,
            Utf8Path::new("books/a.m4b")
        ));
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
        reader_fixture(db, "m4b", b"fake audio").await
    }

    async fn pdf_fixture(db: &livtet_core::data::orm::DatabaseConnection) -> Fixture {
        reader_fixture(db, "pdf", b"%PDF-1.7").await
    }

    /// An edition with one on-disk file. The extension matters: the reader's
    /// format gates compare it case-insensitively.
    async fn reader_fixture(
        db: &livtet_core::data::orm::DatabaseConnection,
        extension: &str,
        contents: &[u8],
    ) -> Fixture {
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
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join(format!("book.{extension}")))
            .expect("utf8 path");
        fs_err::write(&path, contents).expect("fixture file");

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

    async fn mark_format(
        db: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
        format: KnownFormats,
    ) {
        use livtet_core::data::orm::{ActiveModelTrait, IntoActiveModel, Set};
        let mut active = editions::Entity::find_by_id(edition_id)
            .one(db)
            .await
            .expect("query ok")
            .expect("edition present")
            .into_active_model();
        active.format_id = Set(Some(format.into()));
        active.update(db).await.expect("update ok");
    }

    #[tokio::test]
    async fn resolves_audiobook_files() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(&db, fixture.edition_id, None).await;

        let resolved = resolve_reader_file(
            &db,
            fixture.edition_id,
            KnownFormats::Audiobook,
            "audiobook",
        )
        .await
        .expect("audiobook resolves");
        assert_eq!(resolved, fixture.path);
    }

    #[tokio::test]
    async fn rejects_non_audiobook_editions() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;

        resolve_reader_file(
            &db,
            fixture.edition_id,
            KnownFormats::Audiobook,
            "audiobook",
        )
        .await
        .expect_err("an edition without the audiobook format fails closed");
        resolve_reader_file(&db, DbId::new(), KnownFormats::Audiobook, "audiobook")
            .await
            .expect_err("a missing edition fails closed");
    }

    /// The format gate is what keeps the two byte routes from leaking into
    /// each other, so assert it in both directions.
    #[tokio::test]
    async fn resolving_rejects_a_mismatched_format() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(&db, fixture.edition_id, None).await;

        let error =
            resolve_reader_file(&db, fixture.edition_id, KnownFormats::Pdf, PDF_FORMAT_LABEL)
                .await
                .expect_err("an audiobook is not a PDF");
        assert_eq!(error.code, "unsupported");
        assert!(
            error.message.contains("PDF"),
            "the message names the accepted format, got {:?}",
            error.message
        );
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

        let audio = super::super::byte_server::LoopbackServer {
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

        let audio = super::super::byte_server::LoopbackServer {
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

    /// Minimal in-memory EPUB: OPF plus one chapter and one binary.
    fn epub_bytes() -> Vec<u8> {
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
        cursor.into_inner()
    }

    /// [`epub_bytes`] written to a temporary `.epub` file.
    fn fixture() -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
        file.write_all(&epub_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    /// A catalog edition (no format) whose stored file is a valid EPUB.
    async fn epub_fixture(db: &livtet_core::data::orm::DatabaseConnection) -> Fixture {
        reader_fixture(db, "epub", &epub_bytes()).await
    }

    fn loopback() -> super::super::byte_server::LoopbackServer {
        super::super::byte_server::LoopbackServer {
            base_url: "http://127.0.0.1:9".to_string(),
            token: "t".to_string(),
        }
    }

    /// Unwrap without requiring `T: Debug` (readers and plans are not).
    fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
        result.unwrap_or_else(|error| panic!("unexpected error: {error:?}"))
    }

    /// Expect an error without requiring `T: Debug`.
    fn must_fail<T, E>(result: Result<T, E>) -> E {
        match result {
            Ok(_) => panic!("expected an error, got a value"),
            Err(error) => error,
        }
    }

    /// Whether the cache holds no publication (a poisoned cache never counts
    /// as empty, so a test cannot pass by accident).
    fn cache_is_empty(readers: &ReaderCache) -> bool {
        readers.lock().is_ok_and(|cache| cache.is_empty())
    }

    async fn set_title(
        db: &livtet_core::data::orm::DatabaseConnection,
        edition_id: DbId,
        title: Option<&str>,
    ) {
        use livtet_core::data::orm::{ActiveModelTrait, IntoActiveModel, Set};
        let mut active = editions::Entity::find_by_id(edition_id)
            .one(db)
            .await
            .expect("query ok")
            .expect("edition present")
            .into_active_model();
        active.title = Set(title.map(str::to_string));
        active.update(db).await.expect("update ok");
    }

    /// Remove the edition's inventory row, leaving it with no catalog file.
    async fn drop_inventory(db: &livtet_core::data::orm::DatabaseConnection, edition_id: DbId) {
        digital_inventory::Entity::delete_many()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .exec(db)
            .await
            .expect("delete ok");
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
            matches!(
                check_reader_file(None, EPUB_EXTENSION),
                Err(EpubError::NoFile)
            ),
            "missing file path must be NoFile"
        );
    }

    #[test]
    fn unreachable_file_path_is_rejected() {
        let missing = Utf8PathBuf::from(
            std::env::temp_dir()
                .join("livtet-reader-missing-book.epub")
                .to_str()
                .unwrap(),
        );
        assert!(
            !missing.exists(),
            "test precondition: fixture path must not exist"
        );
        assert!(
            matches!(
                check_reader_file(Some(missing.as_str()), EPUB_EXTENSION),
                Err(EpubError::MissingFile)
            ),
            "unreachable file path must be MissingFile"
        );
    }

    #[test]
    fn non_epub_extension_is_rejected_case_insensitively() {
        let dir = tempfile::tempdir().expect("temp dir");
        let pdf = Utf8PathBuf::from(dir.path().to_str().unwrap()).join("book.pdf");
        fs_err::write(&pdf, b"%PDF").expect("write pdf stand-in");
        match check_reader_file(Some(pdf.as_str()), EPUB_EXTENSION) {
            Err(EpubError::UnsupportedFormat { format }) => assert_eq!(format, "pdf"),
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }

        let upper = Utf8PathBuf::from(dir.path().to_str().unwrap()).join("book.EPUB");
        fs_err::write(&upper, b"epub stand-in").expect("write epub stand-in");
        assert!(
            check_reader_file(Some(upper.as_str()), EPUB_EXTENSION).is_ok(),
            ".EPUB must be accepted case-insensitively"
        );
    }

    #[test]
    fn the_pdf_gate_accepts_pdfs_case_insensitively_and_nothing_else() {
        let dir = tempfile::tempdir().expect("temp dir");

        let upper = Utf8PathBuf::from(dir.path().to_str().unwrap()).join("book.PDF");
        fs_err::write(&upper, b"%PDF").expect("write pdf stand-in");
        assert!(
            check_reader_file(Some(upper.as_str()), PDF_EXTENSION).is_ok(),
            ".PDF must be accepted case-insensitively"
        );

        // The gate is symmetric: an EPUB is not openable as a PDF either.
        let epub = Utf8PathBuf::from(dir.path().to_str().unwrap()).join("book.epub");
        fs_err::write(&epub, b"epub stand-in").expect("write epub stand-in");
        match check_reader_file(Some(epub.as_str()), PDF_EXTENSION) {
            Err(EpubError::UnsupportedFormat { format }) => assert_eq!(format, "epub"),
            other => panic!("expected UnsupportedFormat, got {other:?}"),
        }
    }

    #[test]
    fn reader_pdf_window_path_targets_the_nested_spa_route() {
        let id = DbId::new();
        let path = reader_pdf_window_path(&id);
        assert!(
            path.starts_with("reader/pdf/"),
            "window path must target the nested route, got {path}"
        );
        assert!(
            !path.starts_with('/'),
            "window path must be relative, got {path}"
        );
        assert!(
            path.ends_with(id.to_string().as_str()),
            "window path must end with the canonical edition id, got {path}"
        );
    }

    #[tokio::test]
    async fn describes_pdf_publications_with_a_tokenized_url() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = pdf_fixture(&db).await;
        mark_format(&db, fixture.edition_id, KnownFormats::Pdf).await;

        let loopback = super::super::byte_server::LoopbackServer {
            base_url: "http://127.0.0.1:9000".to_string(),
            token: "tok".to_string(),
        };
        let publication = describe_pdf_publication(&db, fixture.edition_id, &loopback)
            .await
            .expect("pdf publication");

        let ReaderPublication::Pdf {
            edition_id,
            pdf_url,
            ..
        } = publication
        else {
            panic!("expected a Pdf publication, got {publication:?}");
        };
        assert_eq!(edition_id, fixture.edition_id.to_string());
        assert_eq!(
            pdf_url,
            format!("http://127.0.0.1:9000/pdf/{}?t=tok", fixture.edition_id)
        );
    }

    /// An edition of the wrong format must not describe as a PDF: routing a
    /// non-PDF here is what the format gate exists to stop.
    #[tokio::test]
    async fn non_pdf_editions_do_not_describe_as_pdfs() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_format(&db, fixture.edition_id, KnownFormats::Audiobook).await;

        let error = describe_pdf_publication(
            &db,
            fixture.edition_id,
            &super::super::byte_server::LoopbackServer {
                base_url: "http://127.0.0.1:9000".to_string(),
                token: "tok".to_string(),
            },
        )
        .await
        .expect_err("an audiobook must not describe as a PDF");
        assert_eq!(error.code, "unsupported");
    }

    #[test]
    fn missing_resource_returns_a_publication_error() {
        let file = fixture();
        let reader =
            livtet_reader::Reader::open(camino::Utf8Path::new(file.path().to_str().unwrap()))
                .expect("fixture opens");
        let err = read_resource_text(&reader, "OEBPS/missing.xhtml").expect_err("missing href");
        assert!(
            matches!(err, EpubError::Publication { .. }),
            "missing resource must be a Publication error, got {err:?}"
        );
    }

    #[test]
    fn resource_text_decodes_utf8() {
        let file = fixture();
        let reader =
            livtet_reader::Reader::open(camino::Utf8Path::new(file.path().to_str().unwrap()))
                .expect("fixture opens");
        let text = read_resource_text(&reader, "OEBPS/ch01.xhtml").expect("chapter reads");
        assert!(text.contains("One"), "unexpected chapter text: {text}");
    }

    #[test]
    fn non_utf8_resource_returns_a_publication_error() {
        let file = fixture();
        let reader =
            livtet_reader::Reader::open(camino::Utf8Path::new(file.path().to_str().unwrap()))
                .expect("fixture opens");
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
    fn reader_audio_window_path_targets_the_nested_spa_route() {
        let id = DbId::new();
        let path = reader_audio_window_path(&id);
        let canonical = id.to_string();
        assert_eq!(path, format!("reader/audio/{canonical}"));
        assert!(
            !path.starts_with('/'),
            "window path must be relative, got {path}"
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
        let reader =
            livtet_reader::Reader::open(camino::Utf8Path::new(file.path().to_str().unwrap()))
                .expect("fixture opens");
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

    #[test]
    fn suffix_ranges_clamp_and_reject_empty_suffixes() {
        assert_eq!(parse_range_header(1000, "bytes=-0"), None);
        assert_eq!(parse_range_header(1000, "bytes=-5000"), Some((0, 999)));
    }

    #[tokio::test]
    async fn byte_ranges_read_exactly_the_inclusive_window() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("bytes.bin");
        fs_err::write(&path, b"0123456789").expect("fixture file");

        let file = tokio::fs::File::open(&path).await.expect("open");
        assert_eq!(read_byte_range(file, 2, 5).await.expect("read"), b"2345");
        let file = tokio::fs::File::open(&path).await.expect("open");
        assert_eq!(read_byte_range(file, 9, 9).await.expect("read"), b"9");
        let file = tokio::fs::File::open(&path).await.expect("open");
        let error = read_byte_range(file, 8, 20)
            .await
            .expect_err("a window past EOF must not be padded");
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn database_errors_map_to_the_database_code() {
        let error = ReaderError::from(livtet_core::data::orm::DbErr::Custom("boom".to_string()));
        assert_eq!(error.code, "database");
    }

    #[test]
    fn library_fence_normalizes_the_books_dir_too() {
        let entry = fence_root().join("books/a.m4b");
        assert!(reader_path_in_library(
            &fence_root().join("./books"),
            &entry
        ));
        assert!(reader_path_in_library(
            &fence_root().join("tmp/../books"),
            &entry
        ));
        assert!(!reader_path_in_library(
            &fence_root().join("tmp/../books"),
            &fence_root().join("tmp/a.m4b")
        ));
        // A books dir that climbs above the root names nothing.
        assert!(!reader_path_in_library(
            &fence_root().join("..").join(".."),
            &fence_root().join("a.m4b")
        ));
    }

    #[tokio::test]
    async fn audiobook_descriptions_reject_other_formats() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = pdf_fixture(&db).await;
        mark_format(&db, fixture.edition_id, KnownFormats::Pdf).await;

        let error = describe_publication(&db, fixture.edition_id, &loopback())
            .await
            .expect_err("a PDF must not describe as an audiobook");
        assert_eq!(error.code, "unsupported");
    }

    #[tokio::test]
    async fn dispatch_routes_editions_by_format() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let audio = audiobook_fixture(&db).await;
        mark_format(&db, audio.edition_id, KnownFormats::Audiobook).await;
        let pdf = pdf_fixture(&db).await;
        mark_format(&db, pdf.edition_id, KnownFormats::Pdf).await;
        let epub = epub_fixture(&db).await;
        mark_format(&db, epub.edition_id, KnownFormats::Epub).await;
        let ebook = epub_fixture(&db).await;
        mark_format(&db, ebook.edition_id, KnownFormats::Ebook).await;
        let unformatted = epub_fixture(&db).await;

        for (fixture, expected) in [
            (&audio, ReaderKind::Audiobook),
            (&pdf, ReaderKind::Pdf),
            (&epub, ReaderKind::Epub),
            (&ebook, ReaderKind::Epub),
            (&unformatted, ReaderKind::Epub),
        ] {
            assert_eq!(
                reader_dispatch(&db, &fixture.edition_id.to_string())
                    .await
                    .expect("query ok"),
                Some((fixture.edition_id, expected)),
            );
        }
        assert_eq!(
            reader_dispatch(&db, &DbId::new().to_string())
                .await
                .expect("query ok"),
            None
        );
        let error = reader_dispatch(&db, "not-an-id")
            .await
            .expect_err("garbage id");
        assert_eq!(error.code, "invalid-id");
    }

    #[tokio::test]
    async fn audiobook_plans_target_the_player_route_without_caching() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(
            &db,
            fixture.edition_id,
            Some(serde_json::json!({ "duration_seconds": 60 })),
        )
        .await;
        let readers = ReaderCache::default();
        let id = fixture.edition_id.to_string();

        let plan = must(plan_reader_window(&db, &readers, &loopback(), &id).await);
        assert_eq!(plan.id, fixture.edition_id);
        assert_eq!(plan.kind, ReaderKind::Audiobook);
        assert_eq!(plan.label, format!("{READER_WINDOW_PREFIX}{id}"));
        assert_eq!(plan.path, reader_audio_window_path(&fixture.edition_id));
        assert_eq!(plan.title, "Book");
        assert!(plan.epub.is_none(), "audiobooks stream over loopback");
        assert!(cache_is_empty(&readers));

        // An untitled audiobook still gets a window title.
        set_title(&db, fixture.edition_id, None).await;
        let plan = must(plan_reader_window(&db, &readers, &loopback(), &id).await);
        assert_eq!(plan.title, "Reader");
    }

    #[tokio::test]
    async fn an_undescribable_audiobook_fails_before_any_window() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = audiobook_fixture(&db).await;
        mark_audiobook(&db, fixture.edition_id, None).await;

        let error = must_fail(
            plan_reader_window(
                &db,
                &ReaderCache::default(),
                &loopback(),
                &fixture.edition_id.to_string(),
            )
            .await,
        );
        assert_eq!(error.code, "invalid");
    }

    #[tokio::test]
    async fn pdf_plans_target_the_pdf_route_and_fail_closed_without_the_file() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = pdf_fixture(&db).await;
        mark_format(&db, fixture.edition_id, KnownFormats::Pdf).await;
        let readers = ReaderCache::default();
        let id = fixture.edition_id.to_string();

        let plan = must(plan_reader_window(&db, &readers, &loopback(), &id).await);
        assert_eq!(plan.kind, ReaderKind::Pdf);
        assert_eq!(plan.label, format!("{READER_WINDOW_PREFIX}{id}"));
        assert_eq!(plan.path, reader_pdf_window_path(&fixture.edition_id));
        assert_eq!(plan.title, "Book");
        assert!(plan.epub.is_none(), "PDFs stream over loopback");

        fs_err::remove_file(&fixture.path).expect("remove pdf");
        let error = must_fail(plan_reader_window(&db, &readers, &loopback(), &id).await);
        assert_eq!(error.code, "missing-file");
        assert!(cache_is_empty(&readers));
    }

    #[tokio::test]
    async fn epub_plans_open_and_cache_the_publication() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = epub_fixture(&db).await;
        let readers = ReaderCache::default();

        let plan = must(
            plan_reader_window(&db, &readers, &loopback(), &fixture.edition_id.to_string()).await,
        );
        assert_eq!(plan.kind, ReaderKind::Epub);
        assert_eq!(
            plan.label,
            format!("{READER_WINDOW_PREFIX}{}", fixture.edition_id)
        );
        assert_eq!(plan.path, reader_pub_window_path(&fixture.edition_id));
        assert_eq!(plan.title, "Book");
        let opened = plan.epub.expect("EPUB plans carry the publication");
        let cached = cached_reader(&readers, &fixture.edition_id).expect("publication cached");
        assert!(
            Arc::ptr_eq(&opened, &cached),
            "the window must serve the publication the plan opened"
        );
    }

    #[tokio::test]
    async fn broken_epub_editions_fail_closed_without_caching() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let no_file = epub_fixture(&db).await;
        drop_inventory(&db, no_file.edition_id).await;
        let missing = epub_fixture(&db).await;
        fs_err::remove_file(&missing.path).expect("remove epub");
        // An unformatted edition routes to the EPUB reader; its file is a PDF.
        let wrong_format = pdf_fixture(&db).await;
        let corrupt = reader_fixture(&db, "epub", b"not a zip archive").await;
        let readers = ReaderCache::default();

        for (edition_id, code) in [
            ("not-an-id".to_string(), "invalid-id"),
            (DbId::new().to_string(), "not-found"),
            (no_file.edition_id.to_string(), "no-file"),
            (missing.edition_id.to_string(), "missing-file"),
            (wrong_format.edition_id.to_string(), "unsupported-format"),
            (corrupt.edition_id.to_string(), "publication"),
        ] {
            let error =
                must_fail(plan_reader_window(&db, &readers, &loopback(), &edition_id).await);
            assert_eq!(error.code, code, "edition {edition_id}: {}", error.message);
        }
        assert!(cache_is_empty(&readers), "failed opens must cache nothing");
    }

    #[tokio::test]
    async fn cached_publications_are_reused_with_a_fresh_title() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = epub_fixture(&db).await;
        let readers = ReaderCache::default();
        let id = fixture.edition_id.to_string();

        let first = must(resolve_reader(&db, &readers, &id).await);
        assert_eq!(first.id, fixture.edition_id);
        assert_eq!(first.title.as_deref(), Some("Book"));

        // A cache hit never touches the disk, but re-reads the catalog title.
        fs_err::remove_file(&fixture.path).expect("remove epub");
        set_title(&db, fixture.edition_id, Some("Renamed")).await;
        let second = must(resolve_reader(&db, &readers, &id).await);
        assert!(Arc::ptr_eq(&first.reader, &second.reader));
        assert_eq!(second.title.as_deref(), Some("Renamed"));

        // Once evicted, the next resolve reopens from disk and fails closed.
        evict_reader(&readers, &fixture.edition_id, &first.reader);
        assert!(cache_is_empty(&readers));
        let error = must_fail(resolve_reader(&db, &readers, &id).await);
        assert!(matches!(error, EpubError::MissingFile), "got {error:?}");
    }

    #[test]
    fn eviction_spares_a_concurrent_replacement() {
        let file = fixture();
        let path = camino::Utf8Path::new(file.path().to_str().unwrap());
        let stale = Arc::new(livtet_reader::Reader::open(path).expect("fixture opens"));
        let replacement = Arc::new(livtet_reader::Reader::open(path).expect("fixture opens"));
        let id = DbId::new();
        let readers = ReaderCache::new(std::collections::HashMap::from([(
            id,
            Arc::clone(&replacement),
        )]));

        evict_reader(&readers, &id, &stale);
        let kept = cached_reader(&readers, &id).expect("replacement survives");
        assert!(Arc::ptr_eq(&kept, &replacement));

        evict_reader(&readers, &id, &replacement);
        assert!(cached_reader(&readers, &id).is_none());
    }

    #[tokio::test]
    async fn publications_describe_each_edition_by_its_reader() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let audio = audiobook_fixture(&db).await;
        mark_audiobook(
            &db,
            audio.edition_id,
            Some(serde_json::json!({ "duration_seconds": 90 })),
        )
        .await;
        let pdf = pdf_fixture(&db).await;
        mark_format(&db, pdf.edition_id, KnownFormats::Pdf).await;
        let epub = epub_fixture(&db).await;
        let no_file = epub_fixture(&db).await;
        drop_inventory(&db, no_file.edition_id).await;
        let readers = ReaderCache::default();
        let loopback = loopback();

        let publication =
            publication_for_edition(&db, &readers, &loopback, &audio.edition_id.to_string())
                .await
                .expect("audiobook describes");
        let Some(ReaderPublication::Audiobook {
            duration_seconds,
            audio_url,
            ..
        }) = publication
        else {
            panic!("expected an audiobook, got {publication:?}");
        };
        assert_eq!(duration_seconds, 90);
        assert_eq!(
            audio_url,
            format!("http://127.0.0.1:9/audio/{}?t=t", audio.edition_id)
        );

        let publication =
            publication_for_edition(&db, &readers, &loopback, &pdf.edition_id.to_string())
                .await
                .expect("pdf describes");
        let Some(ReaderPublication::Pdf { pdf_url, .. }) = publication else {
            panic!("expected a PDF, got {publication:?}");
        };
        assert_eq!(
            pdf_url,
            format!("http://127.0.0.1:9/pdf/{}?t=t", pdf.edition_id)
        );

        let publication =
            publication_for_edition(&db, &readers, &loopback, &epub.edition_id.to_string())
                .await
                .expect("epub describes");
        let Some(ReaderPublication::Epub {
            edition_id,
            title,
            base_url,
            ..
        }) = publication
        else {
            panic!("expected an EPUB, got {publication:?}");
        };
        assert_eq!(edition_id, epub.edition_id.to_string());
        assert_eq!(title.as_deref(), Some("Book"));
        assert_eq!(base_url, reader_base_url(&epub.edition_id));
        assert!(
            cached_reader(&readers, &epub.edition_id).is_some(),
            "describing an EPUB caches it so the reader protocol can serve its resources"
        );

        assert!(
            publication_for_edition(&db, &readers, &loopback, &DbId::new().to_string())
                .await
                .expect("query ok")
                .is_none()
        );
        let error = publication_for_edition(&db, &readers, &loopback, "not-an-id")
            .await
            .expect_err("garbage id");
        assert_eq!(error.code, "invalid-id");
        let error =
            publication_for_edition(&db, &readers, &loopback, &no_file.edition_id.to_string())
                .await
                .expect_err("an EPUB edition without a file");
        assert_eq!(error.code, "no-file");
    }

    #[tokio::test]
    async fn resources_resolve_through_the_edition() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let fixture = epub_fixture(&db).await;
        let readers = ReaderCache::default();
        let id = fixture.edition_id.to_string();

        let text = resource_text_for_edition(&db, &readers, &id, "OEBPS/ch01.xhtml")
            .await
            .expect("chapter reads");
        assert!(text.contains("One"), "unexpected chapter text: {text}");

        let error = resource_text_for_edition(&db, &readers, &id, "OEBPS/missing.xhtml")
            .await
            .expect_err("missing href");
        assert_eq!(error.code, "publication");

        let error =
            resource_text_for_edition(&db, &readers, &DbId::new().to_string(), "OEBPS/ch01.xhtml")
                .await
                .expect_err("unknown edition");
        assert_eq!(error.code, "unknown-edition");
    }

    #[tokio::test]
    async fn bookmarks_round_trip_per_reader_family() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;

        let epub = store_reader_bookmark(
            &db,
            id,
            "epub",
            Some("{\"href\":\"ch1\"}".to_string()),
            None,
            None,
            Some(0.25),
        )
        .await
        .expect("store epub bookmark");
        assert_eq!(epub.kind, "epub");
        assert_eq!(epub.locator.as_deref(), Some("{\"href\":\"ch1\"}"));
        // The number is the page's place in the whole book, not in its chapter.
        assert_eq!(epub.progress, 0.25);
        assert!(epub.created_at > 0.0, "creation time is recorded");

        // PDF and physical books share the page column; only the unit differs.
        let page = store_reader_bookmark(&db, id, "page", None, Some(42), None, None)
            .await
            .expect("store page bookmark");
        assert_eq!(page.kind, "page");
        assert_eq!(page.page_number, Some(42));
        assert_eq!(page.position_seconds, None);

        let listed = fetch_reader_bookmarks(&db, id).await.expect("list");
        assert_eq!(listed.len(), 2, "both bookmarks are listed");
        let ids: Vec<&str> = listed.iter().map(|b| b.id.as_str()).collect();
        assert!(ids.contains(&epub.id.as_str()) && ids.contains(&page.id.as_str()));

        let epub_id = epub.id.parse::<DbId>().expect("bookmark id parses");
        assert!(delete_reader_bookmark(&db, epub_id).await.expect("delete"));
        assert!(
            !delete_reader_bookmark(&db, epub_id)
                .await
                .expect("second delete"),
            "removing a bookmark twice is not an error, just not a change"
        );

        let listed = fetch_reader_bookmarks(&db, id).await.expect("list");
        assert_eq!(listed.len(), 1, "only the page bookmark remains");
        assert_eq!(listed[0].page_number, Some(42));
    }

    #[tokio::test]
    async fn audio_bookmarks_store_seconds_and_need_an_audiobook() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;

        let error = store_reader_bookmark(&db, id, "audio", None, None, Some(90.5), None)
            .await
            .expect_err("audio bookmark on a non-audiobook is unsupported");
        assert_eq!(error.code, "unsupported");

        mark_format(&db, id, KnownFormats::Audiobook).await;
        let audio = store_reader_bookmark(&db, id, "audio", None, None, Some(90.5), None)
            .await
            .expect("store audio bookmark");
        assert_eq!(audio.kind, "timestamp");
        assert_eq!(audio.position_seconds, Some(90.5));
    }

    #[tokio::test]
    async fn a_bookmark_needs_the_position_its_kind_names() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let edition = epub_fixture(&db).await;
        let id = edition.edition_id;

        let error = store_reader_bookmark(&db, id, "pdf", None, None, None, None)
            .await
            .expect_err("a page bookmark needs a page");
        assert_eq!(error.code, "invalid");

        let error = store_reader_bookmark(&db, id, "epub", None, None, None, Some(0.5))
            .await
            .expect_err("an EPUB bookmark needs a locator");
        assert_eq!(error.code, "invalid");

        let error =
            store_reader_bookmark(&db, id, "epub", Some("{}".to_string()), None, None, None)
                .await
                .expect_err("an EPUB bookmark needs the page's place in the book");
        assert_eq!(error.code, "invalid");

        let error = store_reader_bookmark(
            &db,
            id,
            "epub",
            Some("{}".to_string()),
            None,
            None,
            Some(1.5),
        )
        .await
        .expect_err("a fraction outside the book is rejected");
        assert_eq!(error.code, "invalid");

        let error = store_reader_bookmark(&db, id, "seance", None, Some(1), None, None)
            .await
            .expect_err("an unknown kind is rejected");
        assert_eq!(error.code, "invalid-kind");

        let error = store_reader_bookmark(&db, DbId::new(), "pdf", None, Some(1), None, None)
            .await
            .expect_err("an unknown edition is rejected");
        assert_eq!(error.code, "not-found");
    }
}
