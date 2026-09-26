//! Loopback HTTP server for audiobook bytes.
//!
//! WebKitGTK routes `<audio>` through GStreamer, which cannot fetch the
//! app's custom `reader://` scheme — so audio is served over plain HTTP on
//! 127.0.0.1 with an ephemeral port and a per-launch bearer token. The
//! frontend only ever sees the opaque `audio_url` from `reader_publication`.

use std::collections::HashMap;

use poem::{
    EndpointExt, Route, get, handler,
    middleware::AddData,
    web::{Data, Path, Query},
};

use livtet_core::data::orm::DatabaseConnection;
use livtet_types::DbId;

use super::reader::{
    parse_range_header, read_byte_range, reader_path_in_library, resolve_reader_audio,
};

/// The running loopback server: base URL and bearer token for audio URLs.
#[derive(Clone)]
pub struct AudioServer {
    pub base_url: String,
    pub(crate) token: String,
}

/// Minimal server state, kept separate from [`crate::types::AppState`] so
/// tests can build it from a `TestDb` without booting Tauri.
#[derive(Clone)]
pub(crate) struct AudioServerState {
    pub db: DatabaseConnection,
    pub books_dir: camino::Utf8PathBuf,
    pub token: String,
}

/// Bind 127.0.0.1 on an ephemeral port and serve audio.
///
/// The bearer token is 256 random bits, generated per launch and never
/// persisted: any local process can reach loopback, so URLs stay unguessable.
pub(crate) async fn spawn(
    db: DatabaseConnection,
    books_dir: camino::Utf8PathBuf,
) -> std::io::Result<AudioServer> {
    let token = hex::encode(rand::random::<[u8; 32]>());
    let state = AudioServerState {
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
    Ok(AudioServer {
        base_url: format!("http://127.0.0.1:{port}"),
        token,
    })
}

/// Poem route serving `GET /audio/:edition_id?t=<token>`.
pub(crate) fn route(state: AudioServerState) -> impl poem::Endpoint<Output = poem::Response> {
    Route::new()
        .at("/audio/:edition_id", get(serve_audio))
        .with(AddData::new(state))
}

#[handler]
async fn serve_audio(
    Path(edition_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    Data(state): Data<&AudioServerState>,
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
    let path = match resolve_reader_audio(&state.db, edition_id).await {
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
        .header("content-type", "audio/mp4")
        .header("accept-ranges", "bytes")
        .header("content-length", body.len());
    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header("content-range", format!("bytes {start}-{end}/{total_len}"));
    }
    response.body(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use livtet_core::data::orm::EntityTrait;
    use livtet_core::data::{Kind, TestDb};

    const TOKEN: &str = "test-token";

    async fn server_state() -> (TestDb, AudioServerState, tempfile::TempDir, DbId) {
        use livtet_core::data::entities::{digital_inventory, editions, works};
        use livtet_core::data::orm::{ActiveModelTrait, Set};
        use livtet_types::{KnownFormats, now_primitive};

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
        let file_path = books_dir.join("book.m4b");
        std::fs::write(&file_path, b"0123456789abcdef").expect("fixture file");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("Book".to_string())),
            published_date: Set(None),
            format_id: Set(Some(KnownFormats::Audiobook.into())),
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
            file_format: Set(Some("m4b".to_string())),
            notes: Set(None),
            added_at: Set(now),
            updated_at: Set(None),
        }
        .insert(&db)
        .await
        .expect("inventory");

        let state = AudioServerState {
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
