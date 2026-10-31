#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Command-wrapper coverage for `commands::{opds,enrich,calibre,atproto_shelves}`.
//!
//! Every test calls the `#[tauri::command]` wrapper the way Tauri does — with
//! `State<'_, AppState>` taken from a `tauri::test::mock_app()` — against the
//! in-memory fixture in `src/test_fixture.rs`. Feeds, metadata and acquisition
//! files are served from a loopback `poem` server, so nothing leaves the host.
//!
//! What is asserted is what a frontend sees: the flattened DTOs, the rows that
//! land in the catalog, and the JSON shape of the error a command returns.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use livtet_core::data::entities::{
    digital_inventory, edition_identifiers, editions, identifiers, works,
};
use livtet_core::data::orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use livtet_core::data::{Kind, TestDb};
use livtet_desktop_lib::commands::atproto_shelves::{
    ShelvedBook, import_shelved_book, list_shelved_books,
};
use livtet_desktop_lib::commands::calibre::import_calibre_library;
use livtet_desktop_lib::commands::enrich::{apply_enrichment, enrich_library, preview_enrichment};
use livtet_desktop_lib::commands::enrich_merge::EnrichMode;
use livtet_desktop_lib::commands::import::ImportMode;
use livtet_desktop_lib::commands::opds::{
    OpdsAuthInput, OpdsAuthKind, opds_acquire, opds_catalogs_create, opds_catalogs_list,
    opds_catalogs_update, opds_page, opds_search,
};
use livtet_desktop_lib::{AppState, PluginError, mock_state_for_bulk};
use livtet_types::{DbId, now_primitive};
use tauri::Manager;

// ---------------------------------------------------------------- harness ----

#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Reply {
    Reply {
        status: 200,
        content_type,
        body: body.into(),
    }
}

#[derive(Clone)]
struct FixtureState {
    routes: Arc<HashMap<String, Reply>>,
    seen: Arc<Mutex<Vec<String>>>,
}

#[poem::handler]
fn fixture(
    req: &poem::Request,
    poem::web::Data(state): poem::web::Data<&FixtureState>,
) -> poem::Response {
    let uri = req.uri().path_and_query().map_or_else(
        || req.uri().path().to_string(),
        |pq| pq.as_str().to_string(),
    );
    state.seen.lock().unwrap().push(uri.clone());
    let reply = state.routes.get(&uri).cloned().unwrap_or_else(|| Reply {
        status: 404,
        content_type: "text/plain",
        body: Vec::new(),
    });
    poem::Response::builder()
        .status(poem::http::StatusCode::from_u16(reply.status).unwrap())
        .content_type(reply.content_type)
        .body(reply.body)
}

/// A loopback HTTP server answering exact `path?query` routes; everything else
/// is a 404. Every request URI is recorded.
struct FixtureServer {
    origin: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl FixtureServer {
    async fn start(routes: Vec<(&str, Reply)>) -> Self {
        use poem::EndpointExt;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let acceptor = poem::listener::TcpAcceptor::from_std(listener).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let state = FixtureState {
            routes: Arc::new(
                routes
                    .into_iter()
                    // `{origin}` lets a body advertise absolute URLs, the way a
                    // real catalog does for its templated search endpoint.
                    .map(|(path, mut reply)| {
                        // Only text bodies are rewritten; binary replies (an
                        // acquisition EPUB) must reach the client byte-exact.
                        if let Ok(text) = std::str::from_utf8(&reply.body)
                            && text.contains("{origin}")
                        {
                            reply.body = text.replace("{origin}", &origin).into_bytes();
                        }
                        (path.to_string(), reply)
                    })
                    .collect(),
            ),
            seen: seen.clone(),
        };
        tokio::spawn(poem::Server::new_with_acceptor(acceptor).run(fixture.data(state)));
        Self { origin, seen }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

async fn mock_app_for(test_db: &TestDb) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_app();
    app.manage(mock_state_for_bulk(test_db).await);
    app
}

/// The externally-tagged JSON a failing `#[tauri::command]` hands the frontend.
fn error_json(error: &impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(error).expect("a command error serializes")
}

fn no_auth() -> OpdsAuthInput {
    OpdsAuthInput {
        kind: OpdsAuthKind::None,
        username: None,
        password: None,
        token: None,
    }
}

const ATOM: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const ATOM_ACQUISITION: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";
const OPDS_JSON: &str = "application/opds+json";

/// OPDS 1.x root feed: one navigation entry, one publication, a next link.
const ATOM_ROOT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/">
  <id>urn:livtet:test-root</id>
  <title>Test Library</title>
  <updated>2026-01-01T00:00:00Z</updated>
  <link rel="self" href="/atom" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <link rel="next" href="/atom?page=2" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <entry>
    <title>New Releases</title>
    <id>urn:livtet:new</id>
    <link rel="subsection" href="/atom/new" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <title>Dune</title>
    <id>urn:isbn:9780441013593</id>
    <author><name>Frank Herbert</name></author>
    <summary>Desert planet.</summary>
    <dc:language>en</dc:language>
    <published>1965-08-01T00:00:00Z</published>
    <link rel="http://opds-spec.org/image" href="/covers/dune.jpg" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition" href="/books/dune.pdf" type="application/pdf"/>
    <link rel="http://opds-spec.org/acquisition/open-access" href="/books/dune.epub" type="application/epub+zip"/>
  </entry>
</feed>"#;

/// A second OPDS 1.x page, reached through [`ATOM_ROOT`]'s next/subsection links.
const ATOM_PAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/">
  <id>urn:livtet:test-page-2</id>
  <title>Page Two</title>
  <updated>2026-01-02T00:00:00Z</updated>
  <link rel="next" href="/atom/page3" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <entry>
    <title>Dune Messiah</title>
    <id>urn:isbn:9780593098235</id>
    <author><name>Frank Herbert</name></author>
    <dc:language>en</dc:language>
    <link rel="http://opds-spec.org/acquisition/open-access" href="/books/messiah.epub" type="application/epub+zip"/>
  </entry>
</feed>"#;

/// OPDS 2.0 root advertising an absolute, templated search endpoint.
const JSON_ROOT: &str = r#"{
  "metadata": {"title": "JSON Library"},
  "links": [
    {"rel": "self", "href": "/opds2/root.json", "type": "application/opds+json"},
    {"rel": "search", "href": "{origin}/opds2/search{?query}", "type": "application/opds+json", "templated": true}
  ],
  "navigation": [
    {"rel": "current", "href": "/opds2/new.json", "type": "application/opds+json", "title": "New"}
  ]
}"#;

const JSON_SEARCH_RESULTS: &str = r#"{
  "metadata": {"title": "Results for dune messiah"},
  "links": [],
  "publications": [
    {
      "metadata": {"title": "Dune Messiah"},
      "links": [
        {"rel": "http://opds-spec.org/acquisition/open-access", "href": "/books/messiah.epub", "type": "application/epub+zip"}
      ]
    }
  ]
}"#;

// ------------------------------------------------------------------- OPDS ----

#[tokio::test]
async fn create_probes_the_feed_and_persists_the_catalog() {
    let server = FixtureServer::start(vec![("/atom", ok(ATOM, ATOM_ROOT))]).await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;

    let created = opds_catalogs_create(
        "  My Library  ".into(),
        format!("  {}  ", server.url("/atom")),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect("feed probe succeeds");

    // The stored catalog is trimmed, unauthenticated and pointed at the feed.
    assert_eq!(created.title, "My Library");
    assert_eq!(created.feed_url, server.url("/atom"));
    assert_eq!(created.auth_kind, OpdsAuthKind::None);
    assert!(!created.id.is_empty());
    assert!(!created.created_at.is_empty());
    assert_eq!(created.updated_at, created.created_at);
    assert_eq!(
        server.seen(),
        vec!["/atom".to_string()],
        "create probes once"
    );

    // It is really persisted: a fresh read of the store returns it.
    let listed = opds_catalogs_list(app.state::<AppState>()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    assert_eq!(listed[0].title, "My Library");
}

#[tokio::test]
async fn create_rejects_a_blank_title_and_an_unreachable_feed() {
    let server = FixtureServer::start(vec![("/atom", ok(ATOM, ATOM_ROOT))]).await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;

    let blank = opds_catalogs_create(
        "   ".into(),
        server.url("/atom"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("a catalog needs a title");
    let blank = error_json(&blank);
    assert!(blank.get("InvalidInput").is_some(), "{blank}");
    assert_eq!(
        blank["InvalidInput"]["message"], "catalog title is required",
        "{blank}"
    );

    // The probe is the fail-closed gate: a 404 feed never becomes a catalog.
    let missing = opds_catalogs_create(
        "Broken".into(),
        server.url("/no-such-feed"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("the feed probe fails");
    let missing = error_json(&missing);
    assert!(missing.get("Http").is_some(), "{missing}");
    assert_eq!(missing["Http"]["status"], 404, "{missing}");

    assert!(
        opds_catalogs_list(app.state::<AppState>())
            .expect("list")
            .is_empty(),
        "nothing may be persisted when the probe fails"
    );
}

#[tokio::test]
async fn update_renames_the_persisted_catalog_and_rejects_unknown_ids() {
    let server = FixtureServer::start(vec![("/atom", ok(ATOM, ATOM_ROOT))]).await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;

    let created = opds_catalogs_create(
        "Before".into(),
        server.url("/atom"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect("create");

    let updated = opds_catalogs_update(
        created.id.clone(),
        Some("  After  ".into()),
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("update");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.title, "After");
    // No new feed URL means no probe: the title-only update never hits the wire.
    assert_eq!(server.seen(), vec!["/atom".to_string()]);
    assert_eq!(updated.feed_url, created.feed_url);
    assert_eq!(updated.created_at, created.created_at);

    let listed = opds_catalogs_list(app.state::<AppState>()).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "After");

    let missing = opds_catalogs_update(
        "01NOPE".into(),
        Some("Renamed".into()),
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown catalog");
    let missing = error_json(&missing);
    assert!(missing.get("NotFound").is_some(), "{missing}");
    assert_eq!(missing["NotFound"]["id"], "01NOPE", "{missing}");
}

#[tokio::test]
async fn page_follows_a_relative_href_inside_the_catalog_origin() {
    let server = FixtureServer::start(vec![
        ("/atom", ok(ATOM, ATOM_ROOT)),
        ("/atom/page2", ok(ATOM_ACQUISITION, ATOM_PAGE)),
    ])
    .await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;
    let id = opds_catalogs_create(
        "Catalog".into(),
        server.url("/atom"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect("create")
    .id;

    let page = opds_page(id.clone(), "/atom/page2".into(), app.state::<AppState>())
        .await
        .expect("page loads");

    assert_eq!(page.title, "Page Two");
    assert_eq!(page.identifier.as_deref(), Some("urn:livtet:test-page-2"));
    assert_eq!(page.next_href.as_deref(), Some("/atom/page3"));
    assert!(page.navigation.is_empty());
    assert_eq!(page.publications.len(), 1);
    let messiah = &page.publications[0];
    assert_eq!(messiah.title, "Dune Messiah");
    assert_eq!(messiah.authors, vec!["Frank Herbert".to_string()]);
    assert_eq!(messiah.language.as_deref(), Some("en"));
    assert_eq!(
        messiah.acquisition_href.as_deref(),
        Some("/books/messiah.epub")
    );
    assert_eq!(
        messiah.acquisition_type.as_deref(),
        Some("application/epub+zip")
    );
    assert_eq!(
        server.seen(),
        vec!["/atom".to_string(), "/atom/page2".to_string()],
        "the page is fetched from the catalog's own origin"
    );

    // A link that leaves the catalog origin is refused before any request, so
    // the server sees nothing new.
    let foreign = opds_page(
        id.clone(),
        "https://elsewhere.test/feed".into(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("cross-origin page link");
    let foreign = error_json(&foreign);
    assert!(foreign.get("InvalidInput").is_some(), "{foreign}");
    assert_eq!(
        foreign["InvalidInput"]["message"], "page link points outside the catalog",
        "{foreign}"
    );
    assert_eq!(server.seen().len(), 2, "a refused page is never fetched");

    let missing = opds_page(
        "01MISSING".into(),
        "/atom/page2".into(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("unknown catalog");
    let missing = error_json(&missing);
    assert!(missing.get("NotFound").is_some(), "{missing}");
}

#[tokio::test]
async fn search_expands_the_catalog_search_template() {
    let server = FixtureServer::start(vec![
        ("/opds2/root.json", ok(OPDS_JSON, JSON_ROOT)),
        (
            "/opds2/search?query=dune+messiah",
            ok(OPDS_JSON, JSON_SEARCH_RESULTS),
        ),
    ])
    .await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;
    let id = opds_catalogs_create(
        "Catalog".into(),
        server.url("/opds2/root.json"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect("create")
    .id;

    let results = opds_search(id, "dune messiah".into(), app.state::<AppState>())
        .await
        .expect("search runs");

    assert_eq!(results.title, "Results for dune messiah");
    assert_eq!(results.publications.len(), 1);
    assert_eq!(results.publications[0].title, "Dune Messiah");
    assert_eq!(
        results.publications[0].acquisition_href.as_deref(),
        Some("/books/messiah.epub")
    );
    assert_eq!(
        server.seen().last().map(String::as_str),
        Some("/opds2/search?query=dune+messiah")
    );
}

const CONTAINER_XML: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

/// A minimal, importable EPUB 3 carrying `title` and `creator`.
fn epub_bytes(title: &str, creator: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">9780306406157</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>{creator}</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
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
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

#[tokio::test]
async fn acquire_downloads_imports_and_catalogues_the_acquisition_file() {
    let payload = epub_bytes("Dune", "Frank Herbert");
    let server = FixtureServer::start(vec![
        ("/atom", ok(ATOM, ATOM_ROOT)),
        (
            "/books/dune.epub",
            ok("application/epub+zip", payload.clone()),
        ),
    ])
    .await;
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;
    let id = opds_catalogs_create(
        "Catalog".into(),
        server.url("/atom"),
        no_auth(),
        app.state::<AppState>(),
    )
    .await
    .expect("create")
    .id;

    let outcome = opds_acquire(
        id.clone(),
        server.url("/books/dune.epub"),
        app.state::<AppState>(),
    )
    .await
    .expect("acquisition imports");

    assert_eq!(outcome.title, "Dune");
    assert!(!outcome.duplicate, "a fresh download is a new edition");
    assert!(
        outcome.edition_id.parse::<DbId>().is_ok(),
        "the outcome carries the minted edition id"
    );
    assert_eq!(
        server.seen().last().map(String::as_str),
        Some("/books/dune.epub")
    );

    // The download really became catalog rows.
    let db = test_db.state().db_conn();
    let edition_id = outcome.edition_id.parse::<DbId>().unwrap();
    let edition = editions::Entity::find_by_id(edition_id)
        .one(&db)
        .await
        .unwrap()
        .expect("edition row");
    assert_eq!(edition.title.as_deref(), Some("Dune"));
    let inventory = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(&db)
        .await
        .unwrap()
        .expect("inventory row");
    let books_dir = app.state::<AppState>().books_dir.clone();
    let stored = inventory.file_path.expect("the library keeps a copy");
    assert!(
        stored.starts_with(books_dir.as_str()),
        "file {stored} must live under {books_dir}"
    );
    assert_eq!(fs_err::read(&stored).unwrap(), payload);

    // A link that is not an ebook is refused before anything is imported.
    let refused = opds_acquire(id, server.url("/atom"), app.state::<AppState>())
        .await
        .expect_err("an OPDS feed is not an acquisition file");
    let refused = error_json(&refused);
    assert!(refused.get("Feed").is_some(), "{refused}");
}

// ----------------------------------------------------------------- enrich ----

/// Seeds one work + edition, optionally with an ISBN, so the enrichment
/// commands have a real row to read and write.
async fn seed_enrichable_edition(test_db: &TestDb, with_identifier: bool) -> DbId {
    let db = test_db.state().db_conn();
    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set("Untitled Work".into()),
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
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(None),
        published_date: Set(None),
        format_id: Set(None),
        language_id: Set(None),
        notes: Set(None),
        description: Set(None),
        format_metadata: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("edition");

    if with_identifier {
        let identifier_id = DbId::new();
        identifiers::ActiveModel {
            id: Set(identifier_id),
            value: Set("urn:isbn:9780441013593".to_string()),
            kind: Set("isbn".to_string()),
        }
        .insert(&db)
        .await
        .expect("identifier");
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(&db)
        .await
        .expect("edition identifier");
    }

    edition_id
}

/// A mock app whose state points at a temp plugin root and host config, so the
/// enrichment commands launch the real host binary the way the app does.
async fn mock_app_with_enricher(
    test_db: &TestDb,
    plugins_dir: &camino::Utf8Path,
    config: &camino::Utf8Path,
) -> tauri::App<tauri::test::MockRuntime> {
    let mut state = mock_state_for_bulk(test_db).await;
    state.plugin_host_path = host_binary();
    state.plugin_host_config = config.to_owned();
    state.plugins_dir = plugins_dir.to_owned();
    let app = tauri::test::mock_app();
    app.manage(state);
    app
}

/// The host binary cargo builds beside the test binary.
fn host_binary() -> camino::Utf8PathBuf {
    let test_binary = std::env::current_exe().expect("test binary path");
    let profile_dir = test_binary
        .parent()
        .and_then(|path| path.parent())
        .expect("profile dir");
    camino::Utf8PathBuf::from_path_buf(profile_dir.join(if cfg!(windows) {
        "livtet-plugin-host.exe"
    } else {
        "livtet-plugin-host"
    }))
    .expect("utf-8 host path")
}

/// Writes a static enricher (no `http`) plus the host config that forwards it.
fn write_static_enricher(root: &tempfile::TempDir) -> (camino::Utf8PathBuf, camino::Utf8PathBuf) {
    let plugins_dir = camino::Utf8PathBuf::from_path_buf(root.path().join("plugins"))
        .expect("utf-8 plugins path");
    let plugin_dir = plugins_dir.join("static-enricher");
    fs_err::create_dir_all(&plugin_dir).expect("plugin dir");
    fs_err::write(
        plugin_dir.join("plugin.toml"),
        "name = \"static-enricher\"\nversion = \"1.0.0\"\n\n[capabilities.log]\n",
    )
    .expect("manifest");
    fs_err::write(
        plugin_dir.join("init.lua"),
        r#"
local Enricher = {}
Enricher.__index = Enricher
function Enricher.new(config, deps) return setmetatable({}, Enricher) end
function Enricher:sources() return { "isbn" } end
function Enricher:enrich(identity)
  return {
    title = "Enriched Title",
    contributors = {},
    publisher = "Enriched Press",
    subjects = { "Enriched Subject" },
    found = {},
  }
end
return Enricher
"#,
    )
    .expect("plugin lua");

    let config = camino::Utf8PathBuf::from_path_buf(root.path().join("host.toml"))
        .expect("utf-8 config path");
    fs_err::write(
        &config,
        "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"http\"]\n\n[signatures]\nrequired = false\n",
    )
    .expect("host config");
    (plugins_dir, config)
}

#[tokio::test]
async fn a_preview_then_apply_writes_the_edition() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let edition_id = seed_enrichable_edition(&test_db, true).await;
    let root = tempfile::tempdir().expect("temp dir");
    let (plugins_dir, config) = write_static_enricher(&root);
    let app = mock_app_with_enricher(&test_db, &plugins_dir, &config).await;

    let preview = preview_enrichment(app.state::<AppState>(), edition_id.to_string())
        .await
        .expect("preview");
    assert_eq!(preview.sources, vec!["static-enricher".to_string()]);
    assert!(
        preview
            .added
            .iter()
            .any(|change| change.field == "publisher" && change.value == "Enriched Press"),
        "{preview:?}"
    );
    // Nothing is written by a preview.
    let before = editions::Entity::find_by_id(edition_id)
        .one(&test_db.state().db_conn())
        .await
        .expect("read edition")
        .expect("edition row");
    assert_eq!(before.title, None, "a preview leaves the row alone");

    let summary = apply_enrichment(
        app.state::<AppState>(),
        edition_id.to_string(),
        EnrichMode::Fill,
    )
    .await
    .expect("apply");
    assert_eq!(summary.mode, EnrichMode::Fill);
    assert!(summary.fields_added >= 1, "{summary:?}");
    assert!(!summary.cover_stored, "no inventory row, no cover");

    let after = editions::Entity::find_by_id(edition_id)
        .one(&test_db.state().db_conn())
        .await
        .expect("read edition")
        .expect("edition row");
    assert_eq!(after.title.as_deref(), Some("Enriched Title"));
}

#[tokio::test]
async fn preview_refuses_an_edition_with_no_identifiers() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let edition_id = seed_enrichable_edition(&test_db, false).await;
    let app = mock_app_for(&test_db).await;

    let error = preview_enrichment(app.state::<AppState>(), edition_id.to_string())
        .await
        .expect_err("no identifiers, nothing to match on");
    match error {
        PluginError::Invalid { message } => {
            assert!(message.contains("no identifiers"), "{message}");
        }
        other => panic!("expected an invalid-plugin error, got {other:?}"),
    }
}

#[tokio::test]
async fn apply_without_a_preview_is_refused() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let edition_id = seed_enrichable_edition(&test_db, true).await;
    let app = mock_app_for(&test_db).await;

    let error = apply_enrichment(
        app.state::<AppState>(),
        edition_id.to_string(),
        EnrichMode::Overwrite,
    )
    .await
    .expect_err("nothing was previewed");
    match error {
        PluginError::Invalid { message } => {
            assert!(message.contains("preview expired"), "{message}");
        }
        other => panic!("expected an invalid-plugin error, got {other:?}"),
    }
}

#[tokio::test]
async fn bulk_enrich_reports_unavailable_and_takes_a_mode() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;

    let bulk = enrich_library(app.state::<AppState>(), "all".into(), EnrichMode::Union)
        .await
        .expect_err("bulk enrich is not implemented");
    assert!(
        matches!(bulk, PluginError::Unavailable { ref message } if message.contains("bulk enrich")),
        "got {bulk:?}"
    );
}

// ---------------------------------------------------------------- calibre ----

#[tokio::test]
async fn import_calibre_library_wraps_scan_failures_as_import_errors() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;

    // Not a Calibre library at all: rejected before the plugin host is launched.
    let empty = tempfile::tempdir().unwrap();
    let error = import_calibre_library(
        empty.path().to_str().unwrap().to_string(),
        ImportMode::Copy,
        app.state::<AppState>(),
    )
    .await
    .expect_err("no metadata.db");
    let error = error_json(&error);
    assert_eq!(error["code"], "io", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("no metadata.db"),
        "{error}"
    );

    // A metadata.db is present, but the fixture's plugin host is `/dev/null`,
    // so the scan fails at the launch boundary.
    let library = tempfile::tempdir().unwrap();
    fs_err::write(library.path().join("metadata.db"), b"not sqlite").unwrap();
    let error = import_calibre_library(
        library.path().to_str().unwrap().to_string(),
        ImportMode::Copy,
        app.state::<AppState>(),
    )
    .await
    .expect_err("the plugin host cannot launch");
    assert_eq!(error_json(&error)["code"], "importer", "{error:?}");
}

// ---------------------------------------------------------------- atproto ----

/// Point the process at `keyring-core`'s in-process mock so the ATProto store
/// reads no real credential. Returns whether the platform store is usable.
fn install_mock_keyring() -> bool {
    let available = keyring::Entry::store_status().is_ok();
    if available {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
    }
    available
}

fn signed_out() -> ShelvedBook {
    ShelvedBook {
        book_uri: "at://did:plc:test/net.olamaelcu.livtet.book.shelving/1".into(),
        title: "Dune".into(),
        shelf_key: Some("read".into()),
        shelf_name: Some("Finished".into()),
        shelved_at: Some("2026-01-01T00:00:00.000Z".into()),
    }
}

#[tokio::test]
async fn shelved_book_wrappers_report_a_signed_out_account() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app_for(&test_db).await;
    // Before touching the keyring, so no real credential can be read.
    let keyring_available = install_mock_keyring();

    let listed = list_shelved_books(app.state::<AppState>())
        .await
        .expect_err("nobody is signed in");
    let listed = error_json(&listed);
    assert_eq!(listed["code"], "atproto", "{listed}");

    let imported = import_shelved_book(signed_out(), app.state::<AppState>())
        .await
        .expect_err("nobody is signed in");
    let imported = error_json(&imported);
    assert_eq!(imported["code"], "atproto", "{imported}");

    // Both wrappers surface the same mapping. With a working keyring the mock
    // holds no account, so the failure is the signed-out one; a headless
    // keyring fails closed instead, and the message says so.
    if keyring_available {
        let sign_in = "sign in to your ATProto account to import your shelves";
        assert_eq!(listed["message"], sign_in, "{listed}");
        assert_eq!(imported["message"], sign_in, "{imported}");
    } else {
        let message = listed["message"].as_str().unwrap();
        assert!(
            message.starts_with("ATProto keyring unavailable"),
            "{listed}"
        );
    }

    // Nothing reached the catalog: a signed-out wrap is a no-op.
    let db = test_db.state().db_conn();
    assert!(
        editions::Entity::find().all(&db).await.unwrap().is_empty(),
        "no edition may be minted while signed out"
    );
}
