//! Reader-window backend: open EPUB editions in a dedicated window and serve
//! their resources through IPC and the `reader` custom protocol.
//!
//! [`resolve_reader`] is the shared fail-closed entry point: it parses the
//! edition id, resolves the catalog file exactly like `get_edition_detail`,
//! rejects missing or non-EPUB files, and opens (or reuses) the cached
//! [`Reader`](livtet_reader::Reader). The `reader` protocol handler in
//! `crate::run` serves only already-cached publications, so a window can
//! never observe a half-opened book.
//!
//! v1 serves decoded text through [`reader_resource`] for programmatic use by
//! the navigator; binary resources (images, fonts, audio) are served over the
//! `reader` protocol instead of IPC.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use livtet_types::DbId;

use crate::error::ReaderError;
use crate::types::AppState;

/// Label prefix for reader windows; the edition id follows the prefix.
pub const READER_WINDOW_PREFIX: &str = "reader-";

/// An edition's publication payload for the Readium navigator.
///
/// The manifest and positions travel as JSON strings: `serde_json::Value`
/// implements Specta `Type`, but its `Number` arm forbids TypeScript export
/// (BigInt precision), so the structured form cannot leave Rust.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReaderPublication {
    pub base_url: String,
    pub manifest: String,
    pub positions: String,
}

/// An opened (or reused) publication plus the edition metadata the window
/// needs. Kept free of Tauri state where possible so units can stay focused.
pub(crate) struct ResolvedReader {
    pub id: DbId,
    pub title: Option<String>,
    pub reader: Arc<livtet_reader::Reader>,
}

/// Parse an edition id, rejecting garbage before any query or disk access.
pub(crate) fn parse_edition_id(edition_id: &str) -> Result<DbId, ReaderError> {
    edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::unknown_edition(edition_id))
}

/// Validate the catalog file path for reader use: it must exist on disk and
/// carry an `.epub` extension (compared case-insensitively).
pub(crate) fn check_reader_file(file_path: Option<&str>) -> Result<PathBuf, ReaderError> {
    let Some(path) = file_path else {
        return Err(ReaderError::NoFile);
    };
    let fs_path = PathBuf::from(path);
    if !fs_path.exists() {
        return Err(ReaderError::MissingFile);
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
        return Err(ReaderError::unsupported_format(format));
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

/// Look up an already-cached publication without opening anything.
pub(crate) fn cached_lookup(state: &AppState, id: &DbId) -> Option<Arc<livtet_reader::Reader>> {
    state
        .readers
        .lock()
        .ok()
        .and_then(|cache| cache.get(id).cloned())
}

/// Resolve, open, and cache the publication for an edition. Cached readers
/// are reused; every failure mode is fail-closed.
pub(crate) async fn resolve_reader(
    state: &AppState,
    edition_id: &str,
) -> Result<ResolvedReader, ReaderError> {
    let id = parse_edition_id(edition_id)?;
    if let Some(reader) = cached_lookup(state, &id) {
        let title = super::catalog::fetch_edition_detail(&state.db.db_conn(), id)
            .await
            .map_err(ReaderError::publication)?
            .and_then(|detail| detail.title);
        return Ok(ResolvedReader { id, title, reader });
    }

    let detail = super::catalog::fetch_edition_detail(&state.db.db_conn(), id)
        .await
        .map_err(ReaderError::publication)?
        .ok_or_else(|| ReaderError::unknown_edition(edition_id))?;
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
        .map_err(|_| ReaderError::publication("reader cache unavailable"))?
        .insert(id, Arc::clone(&reader));
    Ok(ResolvedReader {
        id,
        title: detail.title,
        reader,
    })
}

/// Assemble the navigator payload for a cached publication.
pub(crate) fn assemble_publication(id: &DbId, reader: &livtet_reader::Reader) -> ReaderPublication {
    let base_url = reader_base_url(id);
    // `Value` serialization is infallible; `null` is a fail-closed fallback.
    let manifest =
        serde_json::to_string(&reader.manifest(&base_url)).unwrap_or_else(|_| "null".to_string());
    let positions =
        serde_json::to_string(&reader.positions()).unwrap_or_else(|_| "null".to_string());
    ReaderPublication {
        base_url,
        manifest,
        positions,
    }
}

/// Decode one publication resource as UTF-8 text.
pub(crate) fn read_resource_text(
    reader: &livtet_reader::Reader,
    href: &str,
) -> Result<String, ReaderError> {
    let Some((_, bytes)) = reader.read(href) else {
        return Err(ReaderError::publication(format!(
            "resource not found: {href}"
        )));
    };
    String::from_utf8(bytes)
        .map_err(|_| ReaderError::publication(format!("resource is not UTF-8 text: {href}")))
}

/// Open the reader window for an edition, focusing it when already open.
///
/// The publication is resolved and cached first so a broken book fails
/// before any window is opened or focused.
#[tauri::command]
#[specta::specta]
pub async fn open_reader(
    edition_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), ReaderError> {
    let resolved = resolve_reader(&state, &edition_id).await?;
    let label = format!("{READER_WINDOW_PREFIX}{}", resolved.id);
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(ReaderError::publication)?;
        return Ok(());
    }
    let title = resolved
        .title
        .clone()
        .unwrap_or_else(|| format!("Reader — {}", resolved.id));
    tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(reader_pub_window_path(&resolved.id).into()),
    )
    .title(title)
    .inner_size(1000.0, 720.0)
    .min_inner_size(480.0, 480.0)
    .build()
    .map_err(ReaderError::publication)?;
    Ok(())
}

/// Fetch the Readium manifest and positions list for an edition.
#[tauri::command]
#[specta::specta]
pub async fn reader_publication(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<ReaderPublication, ReaderError> {
    let resolved = resolve_reader(&state, &edition_id).await?;
    Ok(assemble_publication(&resolved.id, &resolved.reader))
}

/// Read one publication resource as UTF-8 text.
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
    let resolved = resolve_reader(&state, &edition_id).await?;
    read_resource_text(&resolved.reader, &href)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
    fn garbage_edition_id_is_rejected_before_any_query() {
        let err = parse_edition_id("not-a-ulid").expect_err("garbage must fail");
        match err {
            ReaderError::UnknownEdition { id } => assert_eq!(id, "not-a-ulid"),
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
            matches!(check_reader_file(None), Err(ReaderError::NoFile)),
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
                Err(ReaderError::MissingFile)
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
            Err(ReaderError::UnsupportedFormat { format }) => assert_eq!(format, "pdf"),
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
            matches!(err, ReaderError::Publication { .. }),
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
            matches!(err, ReaderError::Publication { .. }),
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
    fn assembled_publication_links_back_to_its_base_url() {
        let file = fixture();
        let reader = livtet_reader::Reader::open(file.path()).expect("fixture opens");
        let id = DbId::new();
        let publication = assemble_publication(&id, &reader);
        let base_url = reader_base_url(&id);
        assert_eq!(publication.base_url, base_url);
        let manifest: serde_json::Value =
            serde_json::from_str(&publication.manifest).expect("manifest is JSON");
        assert_eq!(
            manifest["links"][0]["href"],
            serde_json::json!(format!("{base_url}manifest.json"))
        );
        let positions: serde_json::Value =
            serde_json::from_str(&publication.positions).expect("positions is JSON");
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
    fn reader_errors_map_into_publication_errors() {
        let io = livtet_reader::ReaderError::Package("bad opf".to_string());
        assert!(
            matches!(ReaderError::from(io), ReaderError::Publication { .. }),
            "livtet-reader failures must surface as Publication errors"
        );
    }
}
