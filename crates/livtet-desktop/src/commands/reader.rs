//! Reader window commands and the `reader://` audio scheme backend.
//!
//! Audiobooks play in a dedicated `/reader/[editionId]` window backed by
//! `reader://localhost/editions/{edition_id}/audio`, served with byte ranges
//! so the `<audio>` element can seek a whole-file download. The scheme handler
//! itself lives in `crate::run`; everything testable (URI parsing, range
//! math, edition resolution) lives here.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use livtet_core::data::entities::{digital_inventory, editions};
use livtet_core::data::orm::{ColumnTrait, EntityTrait, QueryFilter};
use livtet_types::{DbId, KnownFormats};

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

/// One audiobook chapter for the player UI.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReaderChapter {
    pub name: String,
    pub audio_start: i32,
    pub audio_end: i32,
}

/// The reader descriptor for one edition. Audiobooks are the only kind today;
/// the EPUB navigator reuses this window and scheme later.
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
}

/// Parse `reader://localhost/editions/{edition_id}/audio`.
pub(crate) fn parse_reader_audio_uri(uri: &str) -> Option<DbId> {
    let rest = uri.strip_prefix("reader://localhost/editions/")?;
    let (id, tail) = rest.split_once('/')?;
    if tail != "audio" {
        return None;
    }
    id.parse::<DbId>().ok()
}

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

/// Describe the edition for the reader window. `Ok(None)` when absent.
///
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
pub(crate) async fn describe_publication(
    db: &livtet_core::data::orm::DatabaseConnection,
    edition_id: DbId,
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
        audio_url: format!("reader://localhost/editions/{edition_id}/audio"),
    }))
}

/// Fetch one edition's reader descriptor. `Ok(None)` when no edition with
/// this id exists.
#[tauri::command]
#[specta::specta]
pub async fn reader_publication(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ReaderPublication>, ReaderError> {
    let id = edition_id
        .parse::<DbId>()
        .map_err(|_| ReaderError::new("invalid-id", format!("invalid edition id: {edition_id}")))?;
    describe_publication(&state.db.db_conn(), id).await
}

/// Serve one `reader://` request: resolve the edition, fence the path inside
/// the library, and answer whole-file (200) or ranged (206) audio bytes.
///
/// Only the requested byte window is ever read, so seeking a large audiobook
/// never loads the whole file.
pub(crate) async fn serve_reader_request(
    app: &AppHandle,
    request: http::Request<Vec<u8>>,
) -> http::Response<std::borrow::Cow<'static, [u8]>> {
    let error_response = |status: http::StatusCode| {
        http::Response::builder()
            .status(status)
            .body(std::borrow::Cow::Borrowed(&[][..]))
            .expect("reader error response uses valid headers")
    };
    let Some(edition_id) = parse_reader_audio_uri(request.uri().to_string().as_str()) else {
        return error_response(http::StatusCode::BAD_REQUEST);
    };
    let state = app.state::<AppState>();
    let db = state.db.db_conn();
    let path = match resolve_reader_audio(&db, edition_id).await {
        Ok(path) => path,
        Err(error) if error.code == "not-found" => {
            return error_response(http::StatusCode::NOT_FOUND);
        }
        Err(_) => return error_response(http::StatusCode::BAD_REQUEST),
    };
    // Fence: the served file must resolve inside the library store.
    let (canonical_file, canonical_books) = match tokio::join!(
        tokio::fs::canonicalize(&path),
        tokio::fs::canonicalize(&state.books_dir)
    ) {
        (Ok(file), Ok(books)) => (file, books),
        _ => return error_response(http::StatusCode::NOT_FOUND),
    };
    if !canonical_file.starts_with(&canonical_books) {
        return error_response(http::StatusCode::FORBIDDEN);
    }
    let file = match tokio::fs::File::open(&canonical_file).await {
        Ok(file) => file,
        Err(_) => return error_response(http::StatusCode::NOT_FOUND),
    };
    let total_len = match file.metadata().await {
        Ok(metadata) => metadata.len(),
        Err(_) => return error_response(http::StatusCode::NOT_FOUND),
    };
    let range = request
        .headers()
        .get(http::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| parse_range_header(total_len, header));
    let (status, start, end) = match range {
        Some((start, end)) => (http::StatusCode::PARTIAL_CONTENT, start, end),
        None if total_len == 0 => (http::StatusCode::OK, 0, 0),
        None => (http::StatusCode::OK, 0, total_len.saturating_sub(1)),
    };
    let body = match read_byte_range(file, start, end).await {
        Ok(body) => body,
        Err(_) => return error_response(http::StatusCode::NOT_FOUND),
    };
    let content_length = body.len();
    let mut response = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, "audio/mp4")
        .header(http::header::ACCEPT_RANGES, "bytes")
        .header(http::header::CONTENT_LENGTH, content_length);
    if status == http::StatusCode::PARTIAL_CONTENT {
        response = response.header(
            http::header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total_len}"),
        );
    }
    response
        .body(std::borrow::Cow::Owned(body))
        .expect("reader response uses valid headers")
}

/// Read the inclusive `[start, end]` byte window of an open file.
async fn read_byte_range(
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

/// Open (or focus) the dedicated reader window for an edition.
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
    // Fail closed before touching windows: only describable editions open.
    let title = describe_publication(&state.db.db_conn(), id)
        .await?
        .ok_or_else(|| ReaderError::new("not-found", "edition not found"))?;
    let title = match &title {
        ReaderPublication::Audiobook { title, .. } => {
            title.clone().unwrap_or_else(|| "Reader".to_string())
        }
    };

    let label = format!("reader-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        window
            .set_focus()
            .map_err(|error| ReaderError::new("window", error))?;
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(format!("reader/{id}").into()))
        .title(title)
        .inner_size(480.0, 800.0)
        .build()
        .map_err(|error| ReaderError::new("window", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::{Kind, TestDb};

    #[test]
    fn parses_reader_audio_uris() {
        let id = DbId::new();
        assert_eq!(
            parse_reader_audio_uri(&format!("reader://localhost/editions/{id}/audio")),
            Some(id)
        );
        assert_eq!(
            parse_reader_audio_uri("reader://localhost/editions/not-an-id/audio"),
            None
        );
        assert_eq!(
            parse_reader_audio_uri(&format!("reader://localhost/editions/{id}/cover")),
            None
        );
        assert_eq!(
            parse_reader_audio_uri("https://example.com/editions/x/audio"),
            None
        );
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

        let publication = describe_publication(&db, fixture.edition_id)
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
                audio_url: format!("reader://localhost/editions/{}/audio", fixture.edition_id),
            }
        );
    }

    #[tokio::test]
    async fn missing_edition_has_no_publication() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let publication = describe_publication(&db, DbId::new())
            .await
            .expect("query ok");
        assert!(publication.is_none());
    }
}
