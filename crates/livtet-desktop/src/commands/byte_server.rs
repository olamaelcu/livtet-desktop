//! Loopback HTTP server for library file bytes.
//!
//! Two consumers need many bytes of a library file that the app's custom
//! `reader://` scheme cannot serve. WebKitGTK routes `<audio>` through
//! GStreamer, which cannot fetch custom schemes at all; and pdf.js wants
//! HTTP range requests so a large scan is never resident in full
//! ([ADR 0037](../../../../doc/adr/0037-render-pdfs-with-pdf-js-in-a-dedicated-reader-window.md)).
//! Both are served over plain HTTP on 127.0.0.1 with an ephemeral port and a
//! per-launch bearer token. The frontend only ever sees the opaque `audio_url`
//! or `pdf_url` from `reader_publication`.

use std::collections::HashMap;

use poem::{
    EndpointExt, Route, get, handler,
    middleware::AddData,
    web::{Data, Path, Query},
};

use livtet_core::data::orm::DatabaseConnection;
use livtet_types::DbId;

use super::reader::{
    parse_range_header, read_byte_range, reader_path_in_library, resolve_reader_file,
};

/// The running loopback server: base URL and bearer token for byte URLs.
#[derive(Clone)]
pub struct LoopbackServer {
    pub base_url: String,
    pub(crate) token: String,
}

/// Minimal server state, kept separate from [`crate::types::AppState`] so
/// tests can build it from a `TestDb` without booting Tauri.
#[derive(Clone)]
pub(crate) struct LoopbackServerState {
    pub db: DatabaseConnection,
    pub books_dir: camino::Utf8PathBuf,
    pub token: String,
}

/// Bind 127.0.0.1 on an ephemeral port and serve library file bytes.
///
/// The bearer token is 256 random bits, generated per launch and never
/// persisted: any local process can reach loopback, so URLs stay unguessable.
pub(crate) async fn spawn(
    db: DatabaseConnection,
    books_dir: camino::Utf8PathBuf,
) -> std::io::Result<LoopbackServer> {
    let token = hex::encode(rand::random::<[u8; 32]>());
    let state = LoopbackServerState {
        db,
        books_dir,
        token: token.clone(),
    };
    // Bind with std first to learn the ephemeral port; poem then serves it.
    // The rebind race is confined to loopback and fails loudly at startup.
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let server = poem::Server::new(poem::listener::TcpListener::bind(format!(
        "127.0.0.1:{port}"
    )));
    tauri::async_runtime::spawn(server.run(route(state)));
    Ok(LoopbackServer {
        base_url: format!("http://127.0.0.1:{port}"),
        token,
    })
}

/// Poem routes serving `GET /audio/:edition_id` and `GET /pdf/:edition_id`,
/// both gated on `?t=<token>`.
///
/// The PDF route also answers `OPTIONS`: pdf.js sends a `Range` header, which
/// is not CORS-safelisted, so every ranged read preflights.
pub(crate) fn route(state: LoopbackServerState) -> impl poem::Endpoint<Output = poem::Response> {
    Route::new()
        .at("/audio/:edition_id", get(serve_audio))
        .at("/pdf/:edition_id", get(serve_pdf).options(preflight_pdf))
        .with(AddData::new(state))
}

/// How one byte route differs from another.
///
/// Everything else — the token check, the library fence, range math — is
/// shared, so a new served format is a new constant, not a new handler body.
struct ByteRoute {
    format: livtet_types::KnownFormats,
    format_label: &'static str,
    content_type: &'static str,
    /// pdf.js is cross-origin JavaScript that reads `content-range` off the
    /// response; `<audio>` is a media element that needs no CORS headers.
    cors: bool,
}

const AUDIO_ROUTE: ByteRoute = ByteRoute {
    format: livtet_types::KnownFormats::Audiobook,
    format_label: "audiobook",
    content_type: "audio/mp4",
    cors: false,
};

const PDF_ROUTE: ByteRoute = ByteRoute {
    format: livtet_types::KnownFormats::Pdf,
    format_label: "PDF",
    content_type: "application/pdf",
    cors: true,
};

/// CORS headers for a cross-origin reader.
///
/// The wildcard origin is safe only because the per-launch bearer token makes
/// these URLs unguessable: any local process can reach loopback, so the token
/// is the access control, not the origin. `expose-headers` is what lets pdf.js
/// read `content-range` at all.
fn cors_headers(builder: poem::ResponseBuilder) -> poem::ResponseBuilder {
    builder
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "GET, OPTIONS")
        .header("access-control-allow-headers", "range")
        .header(
            "access-control-expose-headers",
            "content-range, content-length, accept-ranges",
        )
}

#[handler]
async fn serve_audio(
    Path(edition_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Data(state): Data<&LoopbackServerState>,
    req: &poem::Request,
) -> poem::Response {
    serve_library_bytes(&AUDIO_ROUTE, edition_id, &query, state, req).await
}

#[handler]
async fn serve_pdf(
    Path(edition_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Data(state): Data<&LoopbackServerState>,
    req: &poem::Request,
) -> poem::Response {
    serve_library_bytes(&PDF_ROUTE, edition_id, &query, state, req).await
}

/// Answer the CORS preflight for a ranged PDF read.
///
/// Deliberately unauthenticated: a preflight carries no credentials, and
/// answering it reveals nothing — the GET that follows still needs the token.
#[handler]
async fn preflight_pdf() -> poem::Response {
    cors_headers(poem::Response::builder().status(poem::http::StatusCode::NO_CONTENT))
        .body(poem::Body::empty())
}

/// Serve one library file over HTTP with range support, fail-closed.
async fn serve_library_bytes(
    route: &ByteRoute,
    edition_id: String,
    query: &HashMap<String, String>,
    state: &LoopbackServerState,
    req: &poem::Request,
) -> poem::Response {
    use poem::http::StatusCode;

    let error = |status: StatusCode| {
        poem::Response::builder()
            .status(status)
            .body(poem::Body::empty())
    };
    if query.get("t").map(String::as_str) != Some(state.token.as_str()) {
        return error(StatusCode::UNAUTHORIZED);
    }
    let edition_id = match edition_id.parse::<DbId>() {
        Ok(id) => id,
        Err(_) => return error(StatusCode::BAD_REQUEST),
    };
    let path =
        match resolve_reader_file(&state.db, edition_id, route.format, route.format_label).await {
            Ok(path) => path,
            Err(reader_error) if reader_error.code == "not-found" => {
                return error(StatusCode::NOT_FOUND);
            }
            Err(_) => return error(StatusCode::BAD_REQUEST),
        };
    // Same lexical fence as the former `reader://` handler: the served entry
    // must live in the library store; symlinks are never resolved.
    if !reader_path_in_library(&state.books_dir, &path) {
        return error(StatusCode::FORBIDDEN);
    }
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(_) => return error(StatusCode::NOT_FOUND),
    };
    let total_len = match file.metadata().await {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => return error(StatusCode::FORBIDDEN),
        Err(_) => return error(StatusCode::NOT_FOUND),
    };
    let range = req
        .headers()
        .get(poem::http::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|header| parse_range_header(total_len, header));
    let (status, start, end) = match range {
        Some((start, end)) => (StatusCode::PARTIAL_CONTENT, start, end),
        None if total_len == 0 => (StatusCode::OK, 0, 0),
        None => (StatusCode::OK, 0, total_len.saturating_sub(1)),
    };
    let body = match read_byte_range(file, start, end).await {
        Ok(body) => body,
        Err(_) => return error(StatusCode::NOT_FOUND),
    };
    let mut response = poem::Response::builder()
        .status(status)
        .header("content-type", route.content_type)
        .header("accept-ranges", "bytes")
        .header("content-length", body.len());
    if route.cors {
        response = cors_headers(response);
    }
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header("content-range", format!("bytes {start}-{end}/{total_len}"));
    }
    response.body(body)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use livtet_core::data::orm::EntityTrait;
    use livtet_core::data::{Kind, TestDb};

    const TOKEN: &str = "test-token";

    async fn server_state() -> (TestDb, LoopbackServerState, tempfile::TempDir, DbId) {
        server_state_for(livtet_types::KnownFormats::Audiobook, "m4b").await
    }

    async fn server_state_for(
        format: livtet_types::KnownFormats,
        extension: &str,
    ) -> (TestDb, LoopbackServerState, tempfile::TempDir, DbId) {
        use livtet_core::data::entities::{digital_inventory, editions, works};
        use livtet_core::data::orm::{ActiveModelTrait, Set};
        use livtet_types::now_primitive;

        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
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
        .insert(&db)
        .await
        .expect("work");

        let dir = tempfile::tempdir().expect("temp dir");
        let books_dir =
            camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 path");
        let file_path = books_dir.join(format!("book.{extension}"));
        std::fs::write(&file_path, b"0123456789abcdef").expect("fixture file");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Book".to_string())),
            published_date: Set(None),
            format_id: Set(Some(format.into())),
            language_id: Set(None),
            notes: Set(None),
            description: Set(None),
            format_metadata: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("edition");

        digital_inventory::ActiveModel {
            id: Set(DbId::new()),
            edition_id: Set(edition_id),
            file_path: Set(Some(file_path.to_string())),
            cover_path: Set(None),
            blurhash: Set(None),
            dominant_color: Set(None),
            file_hash: Set(None),
            file_size_bytes: Set(None),
            file_format: Set(Some(extension.to_string())),
            notes: Set(None),
            added_at: Set(now),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("inventory");

        let state = LoopbackServerState {
            db,
            books_dir,
            token: TOKEN.to_string(),
        };
        (test_db, state, dir, edition_id)
    }

    fn audio_url(edition_id: DbId) -> String {
        format!("/audio/{edition_id}?t={TOKEN}")
    }

    #[tokio::test]
    async fn serves_the_whole_file() {
        let (_db, state, _dir, edition_id) = server_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(audio_url(edition_id)).send().await;
        response.assert_status_is_ok();
        response.assert_header("accept-ranges", "bytes");
        response.assert_header("content-length", "16");
        response.assert_text("0123456789abcdef").await;
    }

    #[tokio::test]
    async fn serves_byte_ranges() {
        let (_db, state, _dir, edition_id) = server_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client
            .get(audio_url(edition_id))
            .header("range", "bytes=4-7")
            .send()
            .await;
        assert_eq!(response.0.status(), poem::http::StatusCode::PARTIAL_CONTENT);
        response.assert_header("content-range", "bytes 4-7/16");
        response.assert_text("4567").await;
    }

    #[tokio::test]
    async fn rejects_missing_or_wrong_tokens() {
        let (_db, state, _dir, edition_id) = server_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(format!("/audio/{edition_id}")).send().await;
        response.assert_status(poem::http::StatusCode::UNAUTHORIZED);

        let response = client
            .get(format!("/audio/{edition_id}?t=wrong"))
            .send()
            .await;
        response.assert_status(poem::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_editions_are_not_found() {
        let (_db, state, _dir, _edition_id) = server_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client
            .get(format!("/audio/{}?t={TOKEN}", DbId::new()))
            .send()
            .await;
        response.assert_status(poem::http::StatusCode::NOT_FOUND);
    }

    fn pdf_url(edition_id: DbId) -> String {
        format!("/pdf/{edition_id}?t={TOKEN}")
    }

    async fn pdf_state() -> (TestDb, LoopbackServerState, tempfile::TempDir, DbId) {
        server_state_for(livtet_types::KnownFormats::Pdf, "pdf").await
    }

    #[tokio::test]
    async fn serves_the_whole_pdf() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(pdf_url(edition_id)).send().await;
        response.assert_status_is_ok();
        response.assert_header("content-type", "application/pdf");
        response.assert_header("accept-ranges", "bytes");
        response.assert_header("content-length", "16");
        response.assert_text("0123456789abcdef").await;
    }

    #[tokio::test]
    async fn serves_pdf_byte_ranges() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client
            .get(pdf_url(edition_id))
            .header("range", "bytes=4-7")
            .send()
            .await;
        assert_eq!(response.0.status(), poem::http::StatusCode::PARTIAL_CONTENT);
        response.assert_header("content-range", "bytes 4-7/16");
        response.assert_text("4567").await;
    }

    /// pdf.js reads `content-range` off the response, which cross-origin
    /// JavaScript cannot see unless the header is explicitly exposed.
    #[tokio::test]
    async fn pdf_responses_expose_range_headers_to_the_webview() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(pdf_url(edition_id)).send().await;
        response.assert_header("access-control-allow-origin", "*");
        let exposed = response
            .0
            .headers()
            .get("access-control-expose-headers")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            exposed.contains("content-range") && exposed.contains("accept-ranges"),
            "range headers must be exposed, got {exposed:?}"
        );
    }

    /// A `Range` header is not CORS-safelisted, so pdf.js preflights every
    /// ranged read. Without an `OPTIONS` route each one fails before it starts.
    #[tokio::test]
    async fn pdf_preflight_allows_ranged_reads() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client
            .options(pdf_url(edition_id))
            .header("access-control-request-method", "GET")
            .header("access-control-request-headers", "range")
            .send()
            .await;
        response.assert_status(poem::http::StatusCode::NO_CONTENT);
        response.assert_header("access-control-allow-origin", "*");
        let allowed = response
            .0
            .headers()
            .get("access-control-allow-headers")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        assert!(
            allowed.contains("range"),
            "preflight must allow the range header, got {allowed:?}"
        );
    }

    #[tokio::test]
    async fn rejects_pdf_requests_with_missing_or_wrong_tokens() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(format!("/pdf/{edition_id}")).send().await;
        response.assert_status(poem::http::StatusCode::UNAUTHORIZED);

        let response = client
            .get(format!("/pdf/{edition_id}?t=wrong"))
            .send()
            .await;
        response.assert_status(poem::http::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_editions_have_no_pdf() {
        let (_db, state, _dir, _edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client
            .get(format!("/pdf/{}?t={TOKEN}", DbId::new()))
            .send()
            .await;
        response.assert_status(poem::http::StatusCode::NOT_FOUND);
    }

    /// The two routes are not interchangeable: an audiobook is not a PDF.
    #[tokio::test]
    async fn the_pdf_route_rejects_audiobook_editions() {
        let (_db, state, _dir, edition_id) = server_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(pdf_url(edition_id)).send().await;
        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    /// And the reverse, so a format mix-up cannot leak bytes either way.
    #[tokio::test]
    async fn the_audio_route_rejects_pdf_editions() {
        let (_db, state, _dir, edition_id) = pdf_state().await;
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(audio_url(edition_id)).send().await;
        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn pdfs_outside_the_library_are_forbidden() {
        let (_db, mut state, _dir, edition_id) = pdf_state().await;
        // Fence the server to a sibling directory: the stored path is now
        // outside the library store even though the file exists.
        let elsewhere = tempfile::tempdir().expect("temp dir");
        state.books_dir =
            camino::Utf8PathBuf::from_path_buf(elsewhere.path().to_path_buf()).expect("utf8 path");
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(pdf_url(edition_id)).send().await;
        response.assert_status(poem::http::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn non_audiobook_editions_are_rejected() {
        let (_db, state, _dir, edition_id) = server_state().await;
        {
            use livtet_core::data::entities::editions;
            use livtet_core::data::orm::{ActiveModelTrait, IntoActiveModel, Set};
            let mut active = editions::Entity::find_by_id(edition_id)
                .one(&state.db)
                .await
                .expect("query ok")
                .expect("edition present")
                .into_active_model();
            active.format_id = Set(None);
            active.update(&state.db).await.expect("update ok");
        }
        let client = poem::test::TestClient::new(route(state));

        let response = client.get(audio_url(edition_id)).send().await;
        response.assert_status(poem::http::StatusCode::BAD_REQUEST);
    }
}
