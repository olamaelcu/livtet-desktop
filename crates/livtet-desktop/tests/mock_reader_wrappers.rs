#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Mock-app coverage for the reader Tauri wrappers in `commands/reader.rs`.
//!
//! Every test seeds catalog rows in a real `TestDb`, mounts the in-memory
//! `AppState` fixture on a mock Tauri app, and calls the `#[tauri::command]`
//! wrapper directly. Assertions are behavioural: seeded catalog state in,
//! persisted `reading_progress` / `reading_bookmarks` rows and returned
//! descriptors out.
//!
//! `open_reader` is deliberately absent: its `AppHandle` argument is the real
//! `tauri_runtime_wry::Wry` handle, which a `MockRuntime` app can never
//! produce, so the wrapper is not callable from a mock-app test at all.

use livtet_core::data::entities::{
    digital_inventory, editions, reading_bookmarks, reading_progress, works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
};
use livtet_core::data::{Kind, TestDb};
use livtet_desktop_lib::commands::reader::{
    ReaderChapter, ReaderError, ReaderPosition, ReaderPublication, add_reader_bookmark,
    get_reader_position, list_reader_bookmarks, reader_publication, reader_resource,
    remove_reader_bookmark, save_reader_position,
};
use livtet_desktop_lib::{AppState, mock_state_for_bulk};
use livtet_types::{DbId, KnownFormats, now_primitive};
use serde_json::{Value, json};
use tauri::Manager;

/// The loopback base URL the fixture hands out; every reader URL is asserted
/// against it rather than against a hard-coded port.
const FIXTURE_LOOPBACK: &str = "http://127.0.0.1:0";
/// The loopback token the fixture hands out.
const FIXTURE_TOKEN: &str = "mock";

/// Read the `code` of a `ReaderError` through its serialized contract: the
/// fields are crate-private, but clients see `{ code, message }`.
fn code(error: &ReaderError) -> String {
    serde_json::to_value(error).expect("ReaderError serializes")["code"]
        .as_str()
        .expect("code is a string")
        .to_string()
}

/// Seed a work, an edition, and its inventory row.
///
/// `file_path: None` models a virtual book (catalog metadata, no file);
/// `format: None` leaves the edition formatless, which the reader dispatches
/// to the EPUB backend.
async fn seed_edition(
    db: &DatabaseConnection,
    title: &str,
    format: Option<KnownFormats>,
    format_metadata: Option<Value>,
    file_path: Option<String>,
) -> DbId {
    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set(title.to_string()),
        description: Set(None),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(None),
        preferred_edition_id: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("insert work");

    let edition_id = DbId::new();
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some(title.to_string())),
        published_date: Set(None),
        format_id: Set(format.map(DbId::from)),
        language_id: Set(None),
        notes: Set(None),
        description: Set(None),
        format_metadata: Set(format_metadata),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("insert edition");

    digital_inventory::ActiveModel {
        id: Set(DbId::new()),
        edition_id: Set(edition_id),
        file_path: Set(file_path),
        cover_path: Set(None),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(None),
        file_size_bytes: Set(None),
        // The column is NOT NULL with an `EPUB` default; an explicit NULL is
        // rejected even though the model types it as optional.
        file_format: Set(Some("EPUB".to_string())),
        notes: Set(None),
        added_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("insert inventory");

    edition_id
}

/// The one catalog path a reader wrapper needs: a file that exists on disk
/// with the extension its format expects. The bytes are never parsed for
/// positions or bookmarks.
fn scratch_file(suffix: &str, contents: &[u8]) -> tempfile::NamedTempFile {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("temp file");
    file.write_all(contents).expect("write fixture");
    file.flush().expect("flush fixture");
    file
}

/// UTF-8 path of a fixture file, for the inventory row.
fn utf8_path(file: &tempfile::NamedTempFile) -> String {
    file.path().to_str().expect("utf8 temp path").to_string()
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

/// Minimal in-memory EPUB: OPF plus one chapter and one binary resource.
fn epub_bytes() -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", options).expect("mimetype entry");
        zip.write_all(b"application/epub+zip").expect("mimetype");
        zip.start_file("META-INF/container.xml", options)
            .expect("container entry");
        zip.write_all(CONTAINER.as_bytes()).expect("container");
        zip.start_file("OEBPS/content.opf", options)
            .expect("opf entry");
        zip.write_all(OPF.as_bytes()).expect("opf");
        zip.start_file("OEBPS/ch01.xhtml", options)
            .expect("chapter entry");
        zip.write_all(b"<html><body>One</body></html>")
            .expect("chapter");
        zip.start_file("OEBPS/cover.jpg", options)
            .expect("cover entry");
        zip.write_all(b"\xff\xd8\xff\xfefake-jpeg").expect("cover");
        zip.finish().expect("finish epub");
    }
    cursor.into_inner()
}

#[tokio::test]
async fn reader_publication_describes_an_audiobook() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let edition_id = seed_edition(
        &db,
        "Narrated",
        Some(KnownFormats::Audiobook),
        Some(json!({
            "duration_seconds": 7200,
            "chapters": [{"name": "Intro", "audio_start": 0, "audio_end": 300}],
        })),
        Some("/tmp/does-not-need-to-exist.m4b".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let publication = reader_publication(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("publication")
        .expect("audiobook is described");

    assert_eq!(
        publication,
        ReaderPublication::Audiobook {
            edition_id: edition_id.to_string(),
            title: Some("Narrated".to_string()),
            duration_seconds: 7200,
            chapters: vec![ReaderChapter {
                name: "Intro".to_string(),
                audio_start: 0,
                audio_end: 300,
            }],
            audio_url: format!("{FIXTURE_LOOPBACK}/audio/{edition_id}?t={FIXTURE_TOKEN}"),
        }
    );
}

#[tokio::test]
async fn reader_publication_describes_a_pdf() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let file = scratch_file(".pdf", b"%PDF-1.7 fake");
    let edition_id = seed_edition(
        &db,
        "Scanned",
        Some(KnownFormats::Pdf),
        None,
        Some(utf8_path(&file)),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let publication = reader_publication(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("publication")
        .expect("pdf is described");

    assert_eq!(
        publication,
        ReaderPublication::Pdf {
            edition_id: edition_id.to_string(),
            title: Some("Scanned".to_string()),
            pdf_url: format!("{FIXTURE_LOOPBACK}/pdf/{edition_id}?t={FIXTURE_TOKEN}"),
        }
    );
}

#[tokio::test]
async fn reader_publication_describes_and_caches_an_epub() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let file = scratch_file(".epub", &epub_bytes());
    let edition_id = seed_edition(
        &db,
        "Digital",
        Some(KnownFormats::Epub),
        None,
        Some(utf8_path(&file)),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let publication = reader_publication(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("publication")
        .expect("epub is described");

    let ReaderPublication::Epub {
        edition_id: described_id,
        title,
        base_url,
        manifest,
        positions,
    } = publication
    else {
        panic!("expected an EPUB publication");
    };
    assert_eq!(described_id, edition_id.to_string());
    assert_eq!(title, Some("Digital".to_string()));
    assert!(
        base_url.ends_with(&format!("/{edition_id}/")),
        "base url names the edition: {base_url}"
    );

    let manifest: Value = serde_json::from_str(&manifest).expect("manifest is JSON");
    assert_eq!(
        manifest["metadata"]["title"],
        json!("Reader Command Test"),
        "the manifest is built from the parsed OPF"
    );
    assert!(
        manifest["readingOrder"][0]["href"]
            .as_str()
            .is_some_and(|href| href.contains("ch01.xhtml")),
        "the reading order names the fixture chapter: {manifest}"
    );
    let positions: Value = serde_json::from_str(&positions).expect("positions are JSON");
    assert_eq!(
        positions.as_array().expect("positions array").len(),
        1,
        "one position per reading-order entry"
    );

    // Behavioural side effect: the resolved publication is cached for the
    // `reader` protocol handler, so this is not a re-parse per call.
    let cached = app
        .state::<AppState>()
        .readers
        .lock()
        .expect("cache lock")
        .len();
    assert_eq!(cached, 1, "the EPUB is cached after describing it");

    // The text-resource wrapper rides the same cache.
    let text = reader_resource(
        edition_id.to_string(),
        "ch01.xhtml".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("chapter text");
    assert_eq!(text, "<html><body>One</body></html>");

    let error = reader_resource(
        edition_id.to_string(),
        "nope.xhtml".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("missing resource fails closed");
    assert_eq!(code(&error), "publication");
}

#[tokio::test]
async fn reader_publication_fails_closed() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let virtual_id = seed_edition(&db, "Metadata Only", Some(KnownFormats::Epub), None, None).await;
    let missing_id = seed_edition(
        &db,
        "Deleted File",
        Some(KnownFormats::Epub),
        None,
        Some("/tmp/definitely-not-here.epub".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    // An id that parses but names no edition has no descriptor, not an error.
    let unknown = reader_publication(DbId::new().to_string(), app.state::<AppState>())
        .await
        .expect("an absent edition is not an error");
    assert!(unknown.is_none(), "no descriptor for an absent edition");

    let error = reader_publication("not-a-ulid".to_string(), app.state::<AppState>())
        .await
        .expect_err("unparseable id");
    assert_eq!(code(&error), "invalid-id");

    let error = reader_publication(virtual_id.to_string(), app.state::<AppState>())
        .await
        .expect_err("virtual book has no file");
    assert_eq!(code(&error), "virtual-book");

    let error = reader_publication(missing_id.to_string(), app.state::<AppState>())
        .await
        .expect_err("file recorded but gone");
    assert_eq!(code(&error), "missing-file");
}

#[tokio::test]
async fn save_reader_position_upserts_and_reads_back() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let edition_id = seed_edition(
        &db,
        "Progress",
        Some(KnownFormats::Epub),
        None,
        Some("/tmp/progress.epub".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    assert!(
        get_reader_position(edition_id.to_string(), app.state::<AppState>())
            .await
            .expect("read")
            .is_none(),
        "no position before the first save"
    );

    let annotations = r#"[{"id":"a1","text":"Key point"}]"#;
    save_reader_position(
        edition_id.to_string(),
        "epub".to_string(),
        Some("{\"href\":\"c1\"}".to_string()),
        None,
        None,
        Some(annotations.to_string()),
        app.state::<AppState>(),
    )
    .await
    .expect("save epub position");

    let row = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .one(&db)
        .await
        .expect("query progress")
        .expect("row written");
    assert_eq!(row.progress_unit.as_deref(), Some("epub"));
    assert_eq!(row.last_location.as_deref(), Some("{\"href\":\"c1\"}"));
    assert_eq!(row.annotations.as_deref(), Some(annotations));

    let position = get_reader_position(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("read")
        .expect("stored position");
    assert_eq!(
        position,
        ReaderPosition {
            kind: "epub".to_string(),
            locator: Some("{\"href\":\"c1\"}".to_string()),
            page_number: None,
            position_seconds: None,
            annotations: Some(annotations.to_string()),
        }
    );

    // A PDF save overwrites the same `(edition, format)` row in place and
    // must not blank the annotations owned by the EPUB writer.
    save_reader_position(
        edition_id.to_string(),
        "pdf".to_string(),
        None,
        Some(12),
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("save pdf position");

    let rows = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("query progress");
    assert_eq!(rows.len(), 1, "a second save updates the existing row");
    assert_eq!(rows[0].progress, 12.0);
    assert_eq!(rows[0].progress_unit.as_deref(), Some("pdf"));
    assert_eq!(rows[0].annotations.as_deref(), Some(annotations));

    let position = get_reader_position(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("read")
        .expect("pdf position");
    assert_eq!(
        position,
        ReaderPosition {
            kind: "pdf".to_string(),
            locator: None,
            page_number: Some(12),
            position_seconds: None,
            annotations: Some(annotations.to_string()),
        }
    );

    // An absent edition has no stored position, so reading it is `None`.
    assert!(
        get_reader_position(DbId::new().to_string(), app.state::<AppState>())
            .await
            .expect("read")
            .is_none()
    );

    let error = get_reader_position("not-a-ulid".to_string(), app.state::<AppState>())
        .await
        .expect_err("unparseable edition id");
    assert_eq!(code(&error), "invalid-id");
}

#[tokio::test]
async fn save_reader_position_clamps_audiobook_seconds_to_the_duration() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let edition_id = seed_edition(
        &db,
        "Listen",
        Some(KnownFormats::Audiobook),
        Some(json!({"duration_seconds": 100})),
        Some("/tmp/listen.m4b".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    save_reader_position(
        edition_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(999.0),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("save audiobook position");

    let row = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .one(&db)
        .await
        .expect("query progress")
        .expect("row written");
    assert_eq!(row.progress, 100.0, "position clamps to the duration");
    assert_eq!(row.progress_unit.as_deref(), Some("timestamp"));
    assert_eq!(
        row.last_location
            .as_deref()
            .and_then(|value| value.parse::<f64>().ok()),
        Some(100.0),
        "the textual mirror holds the clamped position"
    );

    let position = get_reader_position(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("read")
        .expect("stored position");
    assert_eq!(position.kind, "audio");
    assert_eq!(position.position_seconds, Some(100.0));
    assert_eq!(position.locator, None);

    // Negative input floors at zero rather than storing a backwards position.
    save_reader_position(
        edition_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(-5.0),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("save negative position");
    let position = get_reader_position(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("read")
        .expect("stored position");
    assert_eq!(position.position_seconds, Some(0.0));
}

#[tokio::test]
async fn save_reader_position_rejects_bad_input() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let epub_id = seed_edition(
        &db,
        "Text",
        Some(KnownFormats::Epub),
        None,
        Some("/tmp/text.epub".to_string()),
    )
    .await;
    let audio_id = seed_edition(
        &db,
        "Talking",
        Some(KnownFormats::Audiobook),
        Some(json!({"duration_seconds": 60})),
        Some("/tmp/talking.m4b".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let error = save_reader_position(
        "not-a-ulid".to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("unparseable edition id");
    assert_eq!(code(&error), "invalid-id");

    let error = save_reader_position(
        DbId::new().to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("absent edition");
    assert_eq!(code(&error), "not-found");

    let error = save_reader_position(
        epub_id.to_string(),
        "video".to_string(),
        None,
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown kind");
    assert_eq!(code(&error), "invalid-kind");

    let error = save_reader_position(
        epub_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(1.0),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("listening progress belongs to audiobooks");
    assert_eq!(code(&error), "unsupported");

    let error = save_reader_position(
        audio_id.to_string(),
        "audio".to_string(),
        None,
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("audio saves require seconds");
    assert_eq!(code(&error), "invalid");

    let error = save_reader_position(
        audio_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(f64::NAN),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("a non-finite position is rejected");
    assert_eq!(code(&error), "invalid");

    let written = reading_progress::Entity::find()
        .all(&db)
        .await
        .expect("query progress");
    assert!(
        written.is_empty(),
        "rejected saves write no progress row: {written:?}"
    );
}

#[tokio::test]
async fn bookmarks_add_list_and_remove_roundtrip() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let edition_id = seed_edition(
        &db,
        "Bookmarked",
        Some(KnownFormats::Epub),
        None,
        Some("/tmp/bookmarked.epub".to_string()),
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let listed = list_reader_bookmarks(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("list");
    assert!(listed.is_empty(), "a fresh edition has no bookmarks");

    // Seed the oldest bookmark directly one hour back so "oldest first" is
    // asserted without depending on clock resolution between two inserts.
    let old_id = DbId::new();
    reading_bookmarks::ActiveModel {
        id: Set(old_id),
        edition_id: Set(edition_id),
        progress: Set(0.1),
        progress_unit: Set(Some("epub".to_string())),
        last_location: Set(Some("{\"href\":\"older\"}".to_string())),
        created_at: Set(now_primitive() - time::Duration::hours(1)),
    }
    .insert(&db)
    .await
    .expect("insert old bookmark");

    let epub = add_reader_bookmark(
        edition_id.to_string(),
        "epub".to_string(),
        Some("{\"href\":\"c2\"}".to_string()),
        None,
        None,
        Some(0.25),
        app.state::<AppState>(),
    )
    .await
    .expect("add epub bookmark");
    assert_eq!(epub.kind, "epub");
    assert_eq!(epub.locator.as_deref(), Some("{\"href\":\"c2\"}"));
    assert_eq!(epub.progress, 0.25);
    assert_eq!(epub.page_number, None);
    assert_eq!(epub.position_seconds, None);
    epub.id
        .parse::<DbId>()
        .expect("the returned id is the row's ULID handle");
    assert!(epub.created_at > 0.0, "created_at is a unix timestamp");

    let page = add_reader_bookmark(
        edition_id.to_string(),
        "pdf".to_string(),
        None,
        Some(42),
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("add page bookmark");
    assert_eq!(page.kind, "pdf");
    assert_eq!(page.page_number, Some(42));
    assert_eq!(page.progress, 42.0);
    assert_eq!(page.locator, None);
    assert_eq!(page.position_seconds, None);

    let listed = list_reader_bookmarks(edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("list");
    assert_eq!(listed.len(), 3);
    assert_eq!(listed[0].id, old_id.to_string(), "oldest bookmark first");
    assert_eq!(listed[0].locator.as_deref(), Some("{\"href\":\"older\"}"));
    assert!(
        listed.iter().any(|bookmark| bookmark.id == epub.id),
        "the epub bookmark is listed"
    );
    assert!(
        listed.iter().any(|bookmark| bookmark.id == page.id),
        "the page bookmark is listed"
    );

    let rows = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("query bookmarks");
    assert_eq!(rows.len(), 3, "every add persisted one row");

    assert!(
        remove_reader_bookmark(epub.id.clone(), app.state::<AppState>())
            .await
            .expect("remove bookmark"),
        "the row existed"
    );
    assert!(
        !remove_reader_bookmark(epub.id.clone(), app.state::<AppState>())
            .await
            .expect("a repeated removal is not an error"),
        "the row was already gone"
    );

    let rows = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("query bookmarks");
    assert_eq!(rows.len(), 2, "one row removed");
    assert!(
        rows.iter().all(|row| row.id.to_string() != epub.id),
        "the removed bookmark is gone"
    );

    let error = remove_reader_bookmark("not-a-ulid".to_string(), app.state::<AppState>())
        .await
        .expect_err("unparseable bookmark id");
    assert_eq!(code(&error), "invalid-id");

    let error = list_reader_bookmarks("not-a-ulid".to_string(), app.state::<AppState>())
        .await
        .expect_err("unparseable edition id");
    assert_eq!(code(&error), "invalid-id");
}

#[tokio::test]
async fn bookmarks_reject_bad_input() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let epub_id = seed_edition(
        &db,
        "Text",
        Some(KnownFormats::Epub),
        None,
        Some("/tmp/text.epub".to_string()),
    )
    .await;
    let audio_id = seed_edition(
        &db,
        "Talking",
        Some(KnownFormats::Audiobook),
        Some(json!({"duration_seconds": 60})),
        Some("/tmp/talking.m4b".to_string()),
    )
    .await;
    let virtual_id = seed_edition(&db, "No File", Some(KnownFormats::Epub), None, None).await;

    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(&test_db).await);

    let error = add_reader_bookmark(
        "not-a-ulid".to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        Some(0.5),
        app.state::<AppState>(),
    )
    .await
    .expect_err("unparseable edition id");
    assert_eq!(code(&error), "invalid-id");

    let error = add_reader_bookmark(
        DbId::new().to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        Some(0.5),
        app.state::<AppState>(),
    )
    .await
    .expect_err("absent edition");
    assert_eq!(code(&error), "not-found");

    let error = add_reader_bookmark(
        virtual_id.to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        Some(0.5),
        app.state::<AppState>(),
    )
    .await
    .expect_err("virtual books have nothing to bookmark");
    assert_eq!(code(&error), "virtual-book");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "video".to_string(),
        None,
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown kind");
    assert_eq!(code(&error), "invalid-kind");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("epub bookmarks need a publication fraction");
    assert_eq!(code(&error), "invalid");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "epub".to_string(),
        Some("{}".to_string()),
        None,
        None,
        Some(1.5),
        app.state::<AppState>(),
    )
    .await
    .expect_err("a fraction above one is rejected");
    assert_eq!(code(&error), "invalid");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "epub".to_string(),
        None,
        None,
        None,
        Some(0.5),
        app.state::<AppState>(),
    )
    .await
    .expect_err("epub bookmarks need a locator");
    assert_eq!(code(&error), "invalid");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "page".to_string(),
        None,
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("page bookmarks need a page number");
    assert_eq!(code(&error), "invalid");

    let error = add_reader_bookmark(
        epub_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(3.0),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("listening bookmarks belong to audiobooks");
    assert_eq!(code(&error), "unsupported");

    let error = add_reader_bookmark(
        audio_id.to_string(),
        "audio".to_string(),
        None,
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("audio bookmarks need seconds");
    assert_eq!(code(&error), "invalid");

    let written = reading_bookmarks::Entity::find()
        .all(&db)
        .await
        .expect("query bookmarks");
    assert!(
        written.is_empty(),
        "rejected bookmarks write no row: {written:?}"
    );

    // The accepted audiobook shape: seconds become the opaque locator too.
    let audio = add_reader_bookmark(
        audio_id.to_string(),
        "audio".to_string(),
        None,
        None,
        Some(12.5),
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("add audiobook bookmark");
    assert_eq!(audio.kind, "timestamp");
    assert_eq!(audio.position_seconds, Some(12.5));
    assert_eq!(audio.progress, 12.5);
    assert_eq!(
        audio.locator, None,
        "audio bookmarks expose seconds, not a locator"
    );
    assert_eq!(audio.page_number, None);

    let row = reading_bookmarks::Entity::find()
        .filter(reading_bookmarks::Column::EditionId.eq(audio_id))
        .one(&db)
        .await
        .expect("query bookmark")
        .expect("row written");
    assert_eq!(row.progress_unit.as_deref(), Some("timestamp"));
    assert_eq!(row.progress, 12.5);
    assert_eq!(
        row.last_location.as_deref(),
        Some("12.5"),
        "the textual mirror holds the playback seconds"
    );
}
