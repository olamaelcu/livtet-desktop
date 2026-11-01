//! KOReader sync server tests: the plugin's own protocol over
//! `poem::test::TestClient`, plus the mapping and position writes underneath.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use livtet_core::data::entities::{
    digital_inventory, editions, reading_bookmarks, reading_progress, works,
};
use livtet_core::data::orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use livtet_core::data::{Kind, TestDb};
use livtet_types::{DbId, KnownFormats, now_primitive};
use poem::test::TestResponse;

use super::{
    Credentials, DEFAULT_PORT, DocumentTarget, Family, KoreaderSyncDeps, KoreaderSyncError,
    KoreaderSyncRuntime, KoreaderSyncSettings, KoreaderSyncSettingsStore, ProgressUpdate,
    StoredKoreaderSync, SyncServerState, apply_progress, ensure_tables, map_documents,
    merge_annotations, partial_md5, read_sync_progress, resolve_document, route,
};

const USER: &str = "livtet";
const PASSWORD: &str = "hunter2";

/// A catalog seeded with one edition and one library file on disk.
struct Fixture {
    _db: TestDb,
    _dirs: Vec<tempfile::TempDir>,
    db: livtet_core::data::orm::DatabaseConnection,
    books_dir: Utf8PathBuf,
    edition_id: DbId,
    inventory_id: DbId,
    document: String,
}

/// The deterministic bytes the fixture writes, so the digest is stable.
fn book_bytes() -> Vec<u8> {
    (0..5000u32).map(|i| (i % 251) as u8).collect()
}

async fn fixture(format: KnownFormats) -> Fixture {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let db = test_db.state().db_conn();

    let dir = tempfile::tempdir().expect("books dir");
    let books_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8 books path");
    let extension = match format {
        KnownFormats::Pdf => "pdf",
        KnownFormats::Audiobook => "m4b",
        _ => "epub",
    };
    let file_path = books_dir.join(format!("book.{extension}"));
    fs_err::write(&file_path, book_bytes()).expect("fixture file");

    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set("Synced Work".to_string()),
        description: Set(None),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(None),
        preferred_edition_id: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("work");

    let edition_id = DbId::new();
    let format_metadata = match format {
        KnownFormats::Pdf => Some(serde_json::json!({ "pages": 100 })),
        KnownFormats::Audiobook => Some(serde_json::json!({ "duration_seconds": 600 })),
        _ => None,
    };
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        // No edition title: the panel's display name comes from the work.
        title: Set(None),
        published_date: Set(None),
        format_id: Set(Some(format.into())),
        language_id: Set(None),
        notes: Set(None),
        description: Set(None),
        format_metadata: Set(format_metadata),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("edition");

    let inventory_id = DbId::new();
    digital_inventory::ActiveModel {
        id: Set(inventory_id),
        edition_id: Set(edition_id),
        file_path: Set(Some(file_path.to_string())),
        cover_path: Set(None),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(None),
        file_size_bytes: Set(Some(book_bytes().len() as i64)),
        file_format: Set(Some(extension.to_string())),
        notes: Set(None),
        added_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("inventory");

    let document = partial_md5(&file_path).expect("digest");

    Fixture {
        _db: test_db,
        _dirs: vec![dir],
        db,
        books_dir,
        edition_id,
        inventory_id,
        document,
    }
}

fn credentials() -> Credentials {
    Credentials {
        username: USER.to_string(),
        userkey: super::md5_hex(PASSWORD.as_bytes()),
    }
}

fn server_state(fixture: &Fixture) -> SyncServerState {
    SyncServerState {
        db: fixture.db.clone(),
        books_dir: fixture.books_dir.clone(),
        credentials: Some(credentials()),
    }
}

fn signed<'a, E>(
    request: poem::test::TestRequestBuilder<'a, E>,
) -> poem::test::TestRequestBuilder<'a, E> {
    request
        .header("x-auth-user", USER)
        .header("x-auth-key", super::md5_hex(PASSWORD.as_bytes()))
}

async fn json(response: TestResponse) -> serde_json::Value {
    let body = response.0.into_body().into_string().await.expect("body");
    serde_json::from_str(&body).expect("json body")
}

/// A settings store that keeps the value in memory instead of on disk.
#[derive(Default)]
struct MemoryStore {
    value: std::sync::Mutex<Option<StoredKoreaderSync>>,
}

impl MemoryStore {
    fn seeded(settings: StoredKoreaderSync) -> Arc<Self> {
        Arc::new(Self {
            value: std::sync::Mutex::new(Some(settings)),
        })
    }
}

impl KoreaderSyncSettingsStore for MemoryStore {
    fn read(&self) -> Option<StoredKoreaderSync> {
        self.value.lock().unwrap().clone()
    }

    fn write(&self, settings: &StoredKoreaderSync) -> Result<(), String> {
        *self.value.lock().unwrap() = Some(settings.clone());
        Ok(())
    }
}

fn free_port() -> u16 {
    let listener =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("ephemeral port");
    let port = listener.local_addr().expect("bound address").port();
    drop(listener);
    port
}

// ---------------------------------------------------------------- digests --

#[test]
fn partial_md5_samples_the_offsets_koreader_reads() {
    let dir = tempfile::tempdir().expect("dir");
    let path = Utf8PathBuf::from_path_buf(dir.path().join("book.epub")).expect("utf8");
    fs_err::write(&path, book_bytes()).expect("write");

    // Cross-checked against an independent implementation of the plugin's
    // `util.partialMD5`: 1024-byte samples at 256, 1024, 4096, stopping at the
    // first offset that reads nothing.
    assert_eq!(
        partial_md5(&path).expect("digest"),
        "212000f2382188ace18597aaaa795ce1"
    );
}

#[test]
fn partial_md5_of_a_file_shorter_than_the_first_sample_is_the_empty_digest() {
    let dir = tempfile::tempdir().expect("dir");
    let path = Utf8PathBuf::from_path_buf(dir.path().join("tiny.epub")).expect("utf8");
    fs_err::write(&path, b"a short book").expect("write");

    // The first sample starts at byte 256, so KOReader hashes nothing at all —
    // the plugin's behaviour, replicated rather than "fixed".
    assert_eq!(
        partial_md5(&path).expect("digest"),
        "d41d8cd98f00b204e9800998ecf8427e"
    );
}

#[test]
fn file_name_digest_hashes_the_name_koreader_reports() {
    assert_eq!(
        super::file_name_digest("book.epub"),
        "03053ffc045564439ff7f2cabb3b58c5"
    );
}

// --------------------------------------------------------------- mapping --

#[tokio::test]
async fn mapping_records_both_checksum_methods_for_a_library_file() {
    let fixture = fixture(KnownFormats::Epub).await;

    let mapped = map_documents(&fixture.db, &fixture.books_dir, 10)
        .await
        .expect("map");
    assert_eq!(mapped, 2, "the partial and the file-name digest");

    for document in [
        fixture.document.clone(),
        super::file_name_digest("book.epub"),
    ] {
        let target = resolve_document(&fixture.db, &fixture.books_dir, &document)
            .await
            .expect("resolve")
            .expect("mapped");
        assert_eq!(target.inventory_id, fixture.inventory_id);
        assert_eq!(target.edition_id, fixture.edition_id);
    }
}

#[tokio::test]
async fn an_unknown_document_resolves_to_nothing() {
    let fixture = fixture(KnownFormats::Epub).await;

    assert!(
        resolve_document(&fixture.db, &fixture.books_dir, "0000deadbeef")
            .await
            .expect("resolve")
            .is_none()
    );
}

#[tokio::test]
async fn files_outside_the_library_are_not_mapped() {
    let fixture = fixture(KnownFormats::Epub).await;
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let outside = Utf8Path::from_path(elsewhere.path()).expect("utf8");

    let mapped = map_documents(&fixture.db, outside, 10).await.expect("map");
    assert_eq!(mapped, 0, "a path outside the fence is never read");
}

// ------------------------------------------------------------- positions --

fn update(document: &str, progress: &str, percentage: f64) -> ProgressUpdate {
    ProgressUpdate {
        document: document.to_string(),
        progress: progress.to_string(),
        percentage,
        device: "Kindle".to_string(),
        device_id: Some("device-1".to_string()),
    }
}

#[tokio::test]
async fn a_pdf_push_writes_the_page_the_readers_use() {
    let fixture = fixture(KnownFormats::Pdf).await;
    map_documents(&fixture.db, &fixture.books_dir, 10)
        .await
        .expect("map");
    let target = DocumentTarget {
        document: fixture.document.clone(),
        inventory_id: fixture.inventory_id,
        edition_id: fixture.edition_id,
    };

    apply_progress(&fixture.db, &target, &update(&fixture.document, "42", 0.42))
        .await
        .expect("push");

    let stored = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(fixture.edition_id))
        .one(&fixture.db)
        .await
        .expect("query")
        .expect("row");
    assert_eq!(stored.progress_unit.as_deref(), Some("pdf"));
    assert_eq!(stored.progress, 42.0);
}

#[tokio::test]
async fn a_pull_replays_the_last_device_push() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let target = DocumentTarget {
        document: fixture.document.clone(),
        inventory_id: fixture.inventory_id,
        edition_id: fixture.edition_id,
    };
    apply_progress(&fixture.db, &target, &update(&fixture.document, "42", 0.42))
        .await
        .expect("push");

    let pulled = read_sync_progress(&fixture.db, &target)
        .await
        .expect("pull")
        .expect("progress");

    assert_eq!(pulled.document, fixture.document);
    assert_eq!(pulled.progress.as_deref(), Some("42"));
    assert_eq!(pulled.percentage, 0.42);
    assert_eq!(pulled.device.as_deref(), Some("Kindle"));
    assert_eq!(pulled.device_id.as_deref(), Some("device-1"));
    assert!(pulled.timestamp > 0);
}

#[tokio::test]
async fn a_livtet_position_further_along_beats_an_older_push() {
    let fixture = fixture(KnownFormats::Epub).await;
    let target = DocumentTarget {
        document: fixture.document.clone(),
        inventory_id: fixture.inventory_id,
        edition_id: fixture.edition_id,
    };
    apply_progress(
        &fixture.db,
        &target,
        &update(&fixture.document, "/body/2", 0.2),
    )
    .await
    .expect("push");

    // The reader in this app moves further along afterwards.
    let locator = serde_json::json!({
        "href": "chapter-3",
        "locations": { "totalProgression": 0.75 }
    })
    .to_string();
    super::reader::store_reader_position(
        &fixture.db,
        fixture.edition_id,
        "epub",
        Some(locator),
        None,
        None,
        None,
    )
    .await
    .expect("livtet save");

    let pulled = read_sync_progress(&fixture.db, &target)
        .await
        .expect("pull")
        .expect("progress");

    assert_eq!(pulled.percentage, 0.75);
    assert_eq!(pulled.progress, None, "a locator is not an xpointer");
    assert_eq!(pulled.device, None, "the position is not a device's");
}

#[tokio::test]
async fn an_epub_push_keeps_the_readers_own_locator() {
    let fixture = fixture(KnownFormats::Epub).await;
    let target = DocumentTarget {
        document: fixture.document.clone(),
        inventory_id: fixture.inventory_id,
        edition_id: fixture.edition_id,
    };
    let locator = serde_json::json!({
        "href": "chapter-1",
        "locations": { "totalProgression": 0.1 }
    })
    .to_string();
    super::reader::store_reader_position(
        &fixture.db,
        fixture.edition_id,
        "epub",
        Some(locator.clone()),
        None,
        None,
        None,
    )
    .await
    .expect("livtet save");

    apply_progress(
        &fixture.db,
        &target,
        &update(&fixture.document, "/body/9", 0.6),
    )
    .await
    .expect("push");

    let stored = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(fixture.edition_id))
        .one(&fixture.db)
        .await
        .expect("query")
        .expect("row");
    assert_eq!(
        stored.last_location.as_deref(),
        Some(locator.as_str()),
        "Livtet's locator is never overwritten by an xpointer"
    );
    assert_eq!(stored.progress, 0.6, "the fraction is mirrored");
}

#[tokio::test]
async fn a_push_without_a_finite_percentage_is_refused() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let target = DocumentTarget {
        document: fixture.document.clone(),
        inventory_id: fixture.inventory_id,
        edition_id: fixture.edition_id,
    };

    let error = apply_progress(
        &fixture.db,
        &target,
        &update(&fixture.document, "1", f64::NAN),
    )
    .await
    .expect_err("NaN is not a position");
    assert_eq!(error.code, "invalid");
}

#[test]
fn annotations_merge_by_id_and_keep_notes_the_push_omits() {
    let stored = r#"[{"id":"a1","text":"first"},{"id":"a2","text":"second"}]"#;
    let incoming = vec![
        serde_json::json!({ "id": "a1", "text": "first, edited" }),
        serde_json::json!({ "id": "a3", "text": "new" }),
    ];

    let merged: Vec<serde_json::Value> =
        serde_json::from_str(&merge_annotations(Some(stored), &incoming).expect("merge"))
            .expect("json");

    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0]["text"], "first, edited");
    assert_eq!(merged[1]["text"], "second");
    assert_eq!(merged[2]["id"], "a3");
}

// -------------------------------------------------------------- the wire --

#[tokio::test]
async fn an_unpaired_server_answers_nothing_but_a_health_probe() {
    let fixture = fixture(KnownFormats::Pdf).await;
    ensure_tables(&fixture.db).await.expect("tables");
    let client = poem::test::TestClient::new(route(SyncServerState {
        db: fixture.db.clone(),
        books_dir: fixture.books_dir.clone(),
        credentials: None,
    }));

    let health = client.get("/healthcheck").send().await;
    assert_eq!(health.0.status(), poem::http::StatusCode::OK);

    let auth = client
        .get("/users/auth")
        .header("x-auth-user", USER)
        .header("x-auth-key", super::md5_hex(PASSWORD.as_bytes()))
        .send()
        .await;
    assert_eq!(auth.0.status(), poem::http::StatusCode::UNAUTHORIZED);
    let body = json(auth).await;
    assert_eq!(body["code"], "unpaired");
}

#[tokio::test]
async fn authentication_checks_both_headers() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let authorised = signed(client.get("/users/auth")).send().await;
    assert_eq!(authorised.0.status(), poem::http::StatusCode::OK);
    assert_eq!(json(authorised).await["authorized"], "OK");

    let wrong_password = client
        .get("/users/auth")
        .header("x-auth-user", USER)
        .header("x-auth-key", super::md5_hex(b"wrong"))
        .send()
        .await;
    assert_eq!(
        wrong_password.0.status(),
        poem::http::StatusCode::UNAUTHORIZED
    );
    assert_eq!(json(wrong_password).await["code"], "unauthorized");

    let wrong_user = client
        .get("/users/auth")
        .header("x-auth-user", "someone")
        .header("x-auth-key", super::md5_hex(PASSWORD.as_bytes()))
        .send()
        .await;
    assert_eq!(wrong_user.0.status(), poem::http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn registration_is_refused_over_http() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let response = client
        .post("/users/create")
        .body(r#"{"username":"someone","password":"x"}"#)
        .send()
        .await;

    assert_eq!(
        response.0.status(),
        poem::http::StatusCode::PAYMENT_REQUIRED
    );
    assert_eq!(json(response).await["code"], "registration-disabled");
}

#[tokio::test]
async fn put_then_get_round_trips_a_position_over_the_plugin_protocol() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let pushed = signed(client.put("/syncs/progress")).body(
        serde_json::json!({
            "document": fixture.document.clone(),
            "progress": "42",
            "percentage": 0.42,
            "device": "Kindle",
            "device_id": "device-1",
        })
        .to_string(),
    );
    let response = pushed.send().await;
    assert_eq!(response.0.status(), poem::http::StatusCode::OK);
    let body = json(response).await;
    assert_eq!(body["document"], fixture.document.as_str());
    assert!(body["timestamp"].as_i64().unwrap_or(0) > 0);

    let pulled = signed(client.get(format!("/syncs/progress/{}", fixture.document)))
        .send()
        .await;
    assert_eq!(pulled.0.status(), poem::http::StatusCode::OK);
    let body = json(pulled).await;
    assert_eq!(body["progress"], "42");
    assert_eq!(body["percentage"], 0.42);
    assert_eq!(body["device"], "Kindle");
    assert_eq!(body["device_id"], "device-1");
}

#[tokio::test]
async fn malformed_and_unknown_pushes_are_refused_by_status() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let malformed = signed(client.put("/syncs/progress")).body("{not json");
    assert_eq!(
        malformed.send().await.0.status(),
        poem::http::StatusCode::BAD_REQUEST
    );

    let unknown = signed(client.put("/syncs/progress")).body(
        serde_json::json!({
            "document": "0000deadbeef",
            "progress": "1",
            "percentage": 0.1,
            "device": "Kindle",
            "device_id": "device-1",
        })
        .to_string(),
    );
    assert_eq!(
        unknown.send().await.0.status(),
        poem::http::StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn the_extension_endpoints_require_credentials() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    for path in [
        "/koreader/status".to_string(),
        "/koreader/books".to_string(),
        format!("/koreader/sync/book/{}", fixture.inventory_id),
    ] {
        let response = client.get(path.clone()).send().await;
        assert_eq!(
            response.0.status(),
            poem::http::StatusCode::UNAUTHORIZED,
            "{path} must not answer without credentials"
        );
    }
}

#[tokio::test]
async fn the_book_extension_merges_notes_and_adds_bookmarks() {
    let fixture = fixture(KnownFormats::Epub).await;
    // A stored note the push does not mention.
    super::reader::store_reader_position(
        &fixture.db,
        fixture.edition_id,
        "epub",
        Some(r#"{"href":"c1","locations":{"totalProgression":0.2}}"#.to_string()),
        None,
        None,
        Some(r#"[{"id":"a1","text":"kept"}]"#.to_string()),
    )
    .await
    .expect("seed position");
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let pushed = signed(client.post(format!("/koreader/sync/book/{}", fixture.inventory_id))).body(
        serde_json::json!({
            "progress": {
                "kind": "epub",
                "annotations": [{ "id": "a2", "text": "from the tablet" }],
            },
            "bookmarks": [{ "kind": "epub", "locator": "{\"href\":\"c1\"}", "fraction": 0.3 }],
        })
        .to_string(),
    );
    let response = pushed.send().await;
    assert_eq!(response.0.status(), poem::http::StatusCode::OK);
    assert_eq!(json(response).await["bookmarks"], 1);

    let read = signed(client.get(format!("/koreader/sync/book/{}", fixture.inventory_id)))
        .send()
        .await;
    let body = json(read).await;
    assert_eq!(
        body["title"], "Synced Work",
        "falls back to the work's title"
    );
    assert_eq!(body["bookmarks"].as_array().expect("bookmarks").len(), 1);

    let notes = body["progress"]["annotations"].as_str().expect("notes");
    let notes: Vec<serde_json::Value> = serde_json::from_str(notes).expect("json");
    assert_eq!(notes.len(), 2, "the stored note survived the merge");
    assert_eq!(notes[0]["id"], "a1");
    assert_eq!(notes[1]["id"], "a2");

    // The locator the reader wrote is kept: the push carried only notes.
    assert_eq!(
        body["progress"]["locator"],
        r#"{"href":"c1","locations":{"totalProgression":0.2}}"#
    );
}

#[tokio::test]
async fn status_and_books_report_what_is_mapped() {
    let fixture = fixture(KnownFormats::Epub).await;
    map_documents(&fixture.db, &fixture.books_dir, 10)
        .await
        .expect("map");
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    let status = signed(client.get("/koreader/status")).send().await;
    let body = json(status).await;
    assert_eq!(body["books"].as_f64(), Some(1.0), "one book, two digests");

    let books = signed(client.get("/koreader/books")).send().await;
    let body = json(books).await;
    let books = body.as_array().expect("array");
    assert_eq!(books.len(), 1, "listed once, not once per digest");
    assert_eq!(books[0]["title"], "Synced Work");
    assert_eq!(books[0]["edition_id"], fixture.edition_id.to_string());
}

// -------------------------------------------------------------- lifecycle --

#[tokio::test]
async fn save_refuses_an_ephemeral_port() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let store = MemoryStore::seeded(StoredKoreaderSync {
        enabled: false,
        port: 8081,
        lan: false,
        username: Some(USER.to_string()),
        userkey: Some(super::md5_hex(PASSWORD.as_bytes())),
    });
    let runtime = KoreaderSyncRuntime::new(store.clone());
    let deps = KoreaderSyncDeps::new(fixture.db.clone(), fixture.books_dir.clone());

    let error: KoreaderSyncError = runtime
        .save(
            KoreaderSyncSettings {
                enabled: true,
                port: 0,
                lan: false,
                username: Some(USER.to_string()),
                password: None,
            },
            &deps,
        )
        .await
        .expect_err("port 0 is refused");

    assert_eq!(error.code, "invalid-port");
    assert_eq!(store.read().expect("settings").port, 8081);
    assert!(!runtime.status(&deps).await.running);
}

#[tokio::test]
async fn saving_a_password_stores_only_its_digest() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let store = Arc::new(MemoryStore::default());
    let runtime = KoreaderSyncRuntime::new(store.clone());
    let deps = KoreaderSyncDeps::new(fixture.db.clone(), fixture.books_dir.clone());

    let saved = runtime
        .save(
            KoreaderSyncSettings {
                enabled: false,
                // A port of the test's own: the machine may already be serving
                // the real thing, and a saved port is bound before it is stored.
                port: free_port(),
                lan: false,
                username: Some(USER.to_string()),
                password: Some(PASSWORD.to_string()),
            },
            &deps,
        )
        .await
        .expect("save");

    let stored = store.read().expect("settings");
    assert_eq!(
        stored.userkey.as_deref(),
        Some(super::md5_hex(PASSWORD.as_bytes()).as_str())
    );
    assert!(saved.settings.paired);
    assert_eq!(saved.settings.username.as_deref(), Some(USER));

    // Saving again without a password keeps the stored digest.
    let port = store.read().expect("settings").port;
    runtime
        .save(
            KoreaderSyncSettings {
                enabled: true,
                port,
                lan: false,
                username: Some(USER.to_string()),
                password: None,
            },
            &deps,
        )
        .await
        .expect("save");
    assert_eq!(
        store.read().expect("settings").userkey,
        stored.userkey,
        "an untouched password field keeps the stored one"
    );
}

#[tokio::test]
async fn an_unsaved_store_reads_as_the_off_by_default_baseline() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let deps = KoreaderSyncDeps::new(fixture.db.clone(), fixture.books_dir.clone());
    let runtime = KoreaderSyncRuntime::new(Arc::new(MemoryStore::default()));

    let status = runtime.status(&deps).await;
    assert_eq!(status.settings.port, DEFAULT_PORT);
    assert!(!status.settings.enabled, "a fresh install opens no socket");
    assert!(!status.settings.paired);
    assert!(!status.running);
}

#[tokio::test]
async fn start_serves_the_plugin_routes_and_stop_unbinds_them() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let port = free_port();
    let store = MemoryStore::seeded(StoredKoreaderSync {
        enabled: false,
        port,
        lan: false,
        username: Some(USER.to_string()),
        userkey: Some(super::md5_hex(PASSWORD.as_bytes())),
    });
    let runtime = KoreaderSyncRuntime::new(store.clone());
    let deps = KoreaderSyncDeps::new(fixture.db.clone(), fixture.books_dir.clone());

    let started = runtime.start(&deps).await.expect("server starts");
    assert!(started.running);
    let url = started
        .url
        .clone()
        .expect("a running server publishes its URL");
    assert!(url.ends_with(&format!(":{port}")), "{url}");

    let client = reqwest::Client::new();
    let health = client
        .get(format!("http://127.0.0.1:{port}/healthcheck"))
        .send()
        .await
        .expect("health check reaches the listener");
    assert_eq!(health.status(), reqwest::StatusCode::OK);

    let auth = client
        .get(format!("http://127.0.0.1:{port}/users/auth"))
        .header("x-auth-user", USER)
        .header("x-auth-key", super::md5_hex(PASSWORD.as_bytes()))
        .send()
        .await
        .expect("auth reaches the listener");
    assert_eq!(auth.status(), reqwest::StatusCode::OK);

    let stopped = runtime.stop(&deps).await.expect("server stops");
    assert!(!stopped.running);
    assert_eq!(stopped.url, None);
    assert!(
        client
            .get(format!("http://127.0.0.1:{port}/healthcheck"))
            .send()
            .await
            .is_err(),
        "a stopped server refuses connections"
    );
}

#[tokio::test]
async fn a_port_that_cannot_bind_leaves_the_running_listener_alone() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let port = free_port();
    let store = MemoryStore::seeded(StoredKoreaderSync {
        enabled: false,
        port,
        lan: false,
        username: Some(USER.to_string()),
        userkey: Some(super::md5_hex(PASSWORD.as_bytes())),
    });
    let runtime = KoreaderSyncRuntime::new(store.clone());
    let deps = KoreaderSyncDeps::new(fixture.db.clone(), fixture.books_dir.clone());
    runtime.start(&deps).await.expect("server starts");

    let held =
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).expect("hold a port");
    let taken = held.local_addr().expect("held address").port();
    let error = runtime
        .save(
            KoreaderSyncSettings {
                enabled: true,
                port: taken,
                lan: false,
                username: Some(USER.to_string()),
                password: None,
            },
            &deps,
        )
        .await
        .expect_err("a taken port is refused");
    assert_eq!(error.code, "bind");

    let status = runtime.status(&deps).await;
    assert!(status.running, "the running listener survived the refusal");
    assert_eq!(store.read().expect("settings").port, port);

    runtime.stop(&deps).await.expect("cleanup");
    drop(held);
}

/// A bookmark written through the extension endpoint is the same row the reader
/// lists, so nothing mirrors state between two paths.
#[tokio::test]
async fn the_extension_bookmarks_are_the_readers_rows() {
    let fixture = fixture(KnownFormats::Pdf).await;
    let client = poem::test::TestClient::new(route(server_state(&fixture)));

    signed(client.post(format!("/koreader/sync/book/{}", fixture.inventory_id)))
        .body(
            serde_json::json!({
                "bookmarks": [{ "kind": "pdf", "page_number": 12 }],
            })
            .to_string(),
        )
        .send()
        .await;

    let rows = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(fixture.edition_id))
        .all(&fixture.db)
        .await
        .expect("query");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].progress, 12.0);

    // A second push of the same bookmark is not a duplicate.
    signed(client.post(format!("/koreader/sync/book/{}", fixture.inventory_id)))
        .body(
            serde_json::json!({
                "bookmarks": [{ "kind": "pdf", "page_number": 12 }],
            })
            .to_string(),
        )
        .send()
        .await;

    let rows = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(fixture.edition_id))
        .all(&fixture.db)
        .await
        .expect("query");
    assert_eq!(rows.len(), 1, "an identical bookmark is not stored twice");
}

#[test]
fn the_server_url_names_an_address_a_device_can_dial() {
    let loopback = super::base_url(
        &StoredKoreaderSync {
            enabled: true,
            port: 8081,
            lan: false,
            username: None,
            userkey: None,
        },
        8081,
    );
    assert_eq!(loopback, "http://127.0.0.1:8081");

    let lan = super::base_url(
        &StoredKoreaderSync {
            enabled: true,
            port: 8081,
            lan: true,
            username: None,
            userkey: None,
        },
        8081,
    );
    assert!(lan.starts_with("http://"), "{lan}");
    assert!(lan.ends_with(":8081"), "{lan}");
    assert!(
        !lan.contains("0.0.0.0"),
        "a wildcard bind is not dialable: {lan}"
    );
}

#[test]
fn desired_address_honours_the_stored_configuration() {
    let off = super::desired_address(&StoredKoreaderSync {
        enabled: false,
        port: 8081,
        lan: true,
        username: None,
        userkey: None,
    });
    assert!(off.is_none());

    let zero = super::desired_address(&StoredKoreaderSync {
        enabled: true,
        port: 0,
        lan: true,
        username: None,
        userkey: None,
    });
    assert!(zero.is_none(), "a hand-edited port 0 must not bind");

    let loopback = super::desired_address(&StoredKoreaderSync {
        enabled: true,
        port: 8081,
        lan: false,
        username: None,
        userkey: None,
    });
    assert_eq!(
        loopback,
        Some((std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 8081))
    );

    let lan = super::desired_address(&StoredKoreaderSync {
        enabled: true,
        port: 8081,
        lan: true,
        username: None,
        userkey: None,
    });
    assert_eq!(
        lan,
        Some((std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 8081))
    );
}

/// The family a book's progress is stored in comes from its catalog row.
#[test]
fn families_follow_the_catalogs_format() {
    let mut edition = editions::Model {
        id: DbId::new(),
        work_id: DbId::new(),
        group_id: None,
        title: None,
        published_date: None,
        format_id: Some(KnownFormats::Pdf.into()),
        language_id: None,
        notes: None,
        description: None,
        format_metadata: None,
        created_at: now_primitive(),
        updated_at: None,
    };
    assert_eq!(super::family_for(&edition, None), Family::Pdf);
    assert_eq!(super::family_for(&edition, Some("epub")), Family::Pdf);

    edition.format_id = Some(KnownFormats::Audiobook.into());
    assert_eq!(super::family_for(&edition, None), Family::Audio);

    edition.format_id = None;
    assert_eq!(super::family_for(&edition, Some("azw3")), Family::Epub);
    assert_eq!(
        super::family_for(&edition, Some("m4b")),
        Family::Audio,
        "an extension is the only format signal a bare row carries"
    );
}
