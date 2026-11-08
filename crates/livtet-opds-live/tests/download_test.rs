//! Integration tests for the OPDS download endpoint and the acquisition
//! links emitted by the feed mapper.
//!
//! These tests exercise the live repository (via a real SQLite test DB
//! provided by [`livtet_core::test_db::TestDb`]) and the live HTTP
//! route table (via [`poem::test::TestClient`]). They cover:
//!
//! - [`OpdsRepository::get_linked_file`] — happy path, broken symlink,
//!   missing file, garbage work_id, inventory_id selector.
//! - [`OpdsRepository::get_linked_file`] — broken symlink triggers
//!   pruning of the `digital_inventory.file_path` column.
//! - `GET /publications/:work_id/download` — serves bytes with correct
//!   `Content-Type`, `Content-Disposition`, `Content-Length`.
//! - `GET /publications/:work_id/download` — 404 when no file is
//!   linked.
//! - [`DefaultOpdsMapper`] — feeds include `acquisition` links for
//!   editions with linked files.
//!
//! The HTTP tests use a thin in-crate handler that wraps the
//! production [`OpdsService`], mirroring the production
//! [`livtet_tauri::commands::opds::BridgeHandler`] so behaviour stays
//! aligned without taking a Tauri state dependency.

// Test code reaches for `std::fs::write` / `std::fs::remove_file` and
// `std::os::unix::fs::symlink` directly. Clippy's workspace
// `disallowed_methods` would flag each one; the allow below keeps the
// tests focused on behaviour rather than ceremony around `fs_err` /
// `camino`. Mirrors the explicit `#[allow]` on
// `livtet_core::crud::file_info::compute_and_cache_file_hash`.
#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use std::sync::Arc;

use async_trait::async_trait;
use livtet_core::data::{
    Kind, TestDb,
    entities::{digital_inventory, editions},
};
use livtet_opds_live::LivtetOpdsRepository;
use livtet_opds_server::{
    DefaultOpdsMapper, OpdsHandler, OpdsRepository, OpdsService, SharedOpdsHandler,
    SharedOpdsRepository,
};
use livtet_opds_types::{Feed, OpdsError, Publication};
use livtet_types::{DbId, now_primitive};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

mod common;
use common::{CreateWorkInput, create_work};

/// Build a fresh Tantivy `SearchIndex` in a temp directory and `reindex`
/// it against the supplied database.
async fn fresh_search_index(
    db: &sea_orm::DatabaseConnection,
) -> (
    Arc<livtet_search::SearchIndex>,
    camino_tempfile::Utf8TempDir,
) {
    let dir = camino_tempfile::tempdir().expect("tempdir");
    let index = livtet_search::SearchIndex::open(dir.path()).expect("open index");
    index.reindex(db).await.expect("reindex");
    (Arc::new(index), dir)
}

/// Thin `OpdsHandler` impl used by the HTTP tests. Delegates every
/// method to the same [`OpdsService`] used in production, so the test
/// stays faithful to the live wiring.
struct TestHandler {
    service: OpdsService,
}

#[async_trait]
impl OpdsHandler for TestHandler {
    async fn get_root_feed(&self) -> Result<Feed, OpdsError> {
        self.service.get_feed().await
    }

    async fn get_publication(&self, id: &str) -> Result<Publication, OpdsError> {
        self.service.get_publication(id).await
    }

    async fn search(&self, query: &str) -> Result<Feed, OpdsError> {
        self.service.search(query).await
    }

    async fn get_navigation_authors(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_authors().await
    }

    async fn get_navigation_tags(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_tags().await
    }

    async fn get_navigation_genres(&self) -> Result<Feed, OpdsError> {
        self.service.navigation.handle_genres().await
    }

    async fn get_linked_file(
        &self,
        work_id: &str,
        inventory_id: Option<&str>,
    ) -> Result<(Vec<u8>, String, String), OpdsError> {
        self.service.get_linked_file(work_id, inventory_id).await
    }
}

/// Build a [`SharedOpdsHandler`] backed by a fresh [`TestDb`].
async fn make_handler(test_db: &TestDb) -> SharedOpdsHandler {
    let (index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo: SharedOpdsRepository =
        Arc::new(LivtetOpdsRepository::from_state(&test_db.state(), index));
    let service = OpdsService::new(repo, "http://localhost/opds".to_string());
    Arc::new(TestHandler { service })
}

/// Build the route table from a [`SharedOpdsHandler`] without auth.
fn make_app(handler: SharedOpdsHandler) -> poem::endpoint::BoxEndpoint<'static, poem::Response> {
    livtet_opds_server::routes::make_routes(handler, None)
}

/// Write `bytes` to `dir/name` and return the resulting path.
fn write_file(dir: &camino::Utf8Path, name: &str, bytes: &[u8]) -> camino::Utf8PathBuf {
    let p = dir.join(name);
    std::fs::write(p.as_std_path(), bytes).expect("write file");
    p
}

/// Insert a work with the given title.
async fn seed_work(repo: &LivtetOpdsRepository, title: &str) -> DbId {
    create_work(
        repo.db(),
        CreateWorkInput {
            title: title.into(),
            description: None,
            work_authors: vec![],
            work_tags: vec![],
            work_genres: vec![],
            work_subjects: vec![],
            work_publishers: vec![],
            preferred_edition_id: None,
        },
    )
    .await
    .expect("create work")
    .work
    .id
}

/// Insert a bare edition row for `work_id`.
async fn seed_edition(repo: &LivtetOpdsRepository, work_id: DbId) -> DbId {
    let id = DbId::new();
    editions::ActiveModel {
        id: Set(id),
        work_id: Set(work_id),
        created_at: Set(now_primitive()),
        updated_at: Set(Some(now_primitive())),
        ..Default::default()
    }
    .insert(repo.db())
    .await
    .expect("insert edition")
    .id
}

/// Insert a `digital_inventory` row linking `edition_id` to `file_path`.
async fn seed_inventory(repo: &LivtetOpdsRepository, edition_id: DbId, file_path: &str) -> DbId {
    let id = DbId::new();
    digital_inventory::ActiveModel {
        id: Set(id),
        edition_id: Set(edition_id),
        file_path: Set(Some(file_path.to_string())),
        cover_path: Set(None),
        file_hash: Set(None),
        file_size_bytes: Set(None),
        notes: Set(None),
        ..Default::default()
    }
    .insert(repo.db())
    .await
    .expect("insert inventory")
    .id
}

// ---------------------------------------------------------------------------
// Repository: get_linked_file
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_linked_file_returns_bytes_for_valid_file() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "book.epub", b"epub-content");
    let work_id = seed_work(&repo, "Linked Book").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, path.as_str()).await;

    let (bytes, mime_type, filename) = repo
        .get_linked_file(&work_id.to_string(), None)
        .await
        .expect("get_linked_file");

    assert_eq!(bytes, b"epub-content");
    assert_eq!(mime_type, "application/epub+zip");
    assert_eq!(filename, "book.epub");
    let _ = inv_id; // kept for clarity — inventory_id is exercised in the next test
}

#[tokio::test]
async fn get_linked_file_picks_specific_inventory_when_requested() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let epub_path = write_file(tmp.path(), "book.epub", b"epub-content");
    let pdf_path = write_file(tmp.path(), "book.pdf", b"pdf-content");
    let work_id = seed_work(&repo, "Two Format Book").await;
    let epub_edition = seed_edition(&repo, work_id).await;
    let pdf_edition = seed_edition(&repo, work_id).await;
    let _ = seed_inventory(&repo, epub_edition, epub_path.as_str()).await;
    let pdf_inv = seed_inventory(&repo, pdf_edition, pdf_path.as_str()).await;

    let (bytes, mime_type, _filename) = repo
        .get_linked_file(&work_id.to_string(), Some(&pdf_inv.to_string()))
        .await
        .expect("get_linked_file pdf");

    assert_eq!(bytes, b"pdf-content");
    assert_eq!(mime_type, "application/pdf");
}

#[tokio::test]
async fn get_linked_file_returns_missing_field_when_no_file_linked() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let work_id = seed_work(&repo, "Unlinked Book").await;

    let err = repo
        .get_linked_file(&work_id.to_string(), None)
        .await
        .expect_err("expected MissingField");
    assert!(matches!(err, OpdsError::MissingField(_)), "got {err:?}");
}

#[tokio::test]
async fn get_linked_file_returns_missing_field_for_invalid_inventory_id() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "book.epub", b"x");
    let work_id = seed_work(&repo, "Linked").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let _ = seed_inventory(&repo, edition_id, path.as_str()).await;

    let bogus_inv = DbId::new();
    let err = repo
        .get_linked_file(&work_id.to_string(), Some(&bogus_inv.to_string()))
        .await
        .expect_err("expected MissingField");
    assert!(matches!(err, OpdsError::MissingField(_)), "got {err:?}");
}

#[tokio::test]
async fn get_linked_file_returns_missing_field_for_garbage_work_id() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let err = repo
        .get_linked_file("not-a-ulid", None)
        .await
        .expect_err("expected MissingField for garbage id");
    assert!(matches!(err, OpdsError::MissingField(_)), "got {err:?}");
}

#[tokio::test]
async fn get_linked_file_returns_missing_field_for_missing_work() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let missing = DbId::new();
    let err = repo
        .get_linked_file(&missing.to_string(), None)
        .await
        .expect_err("expected MissingField");
    assert!(matches!(err, OpdsError::MissingField(_)), "got {err:?}");
}

#[tokio::test]
#[cfg(unix)]
async fn get_linked_file_returns_missing_field_for_broken_symlink_and_prunes() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let src = write_file(tmp.path(), "source.epub", b"target-bytes");
    let linked_dir = tmp.path().join("linked-books");
    std::fs::create_dir_all(&linked_dir).expect("mkdir");
    let linked = linked_dir.join("01HABCDEFGHJKMNPQRSTVWXYZ.epub");
    std::os::unix::fs::symlink(&src, &linked).expect("symlink");
    let linked_str = linked.as_str().to_string();

    let work_id = seed_work(&repo, "Broken Link").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, &linked_str).await;

    // Delete the symlink target to break the link.
    std::fs::remove_file(&src).expect("remove target");

    let err = repo
        .get_linked_file(&work_id.to_string(), None)
        .await
        .expect_err("expected MissingField");
    assert!(matches!(err, OpdsError::MissingField(_)), "got {err:?}");

    // Pruning helper should have nulled the file_path column.
    let row = digital_inventory::Entity::find_by_id(inv_id)
        .one(repo.db())
        .await
        .expect("query inventory")
        .expect("row present");
    assert!(
        row.file_path.is_none(),
        "pruning helper should null file_path, got {:?}",
        row.file_path
    );
}

// ---------------------------------------------------------------------------
// HTTP: GET /publications/:work_id/download
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_route_serves_file_with_correct_headers() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let handler = make_handler(&test_db).await;
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "pride.epub", b"PRIDE-AND-PREJUDICE");
    let work_id = seed_work(&repo, "Pride and Prejudice").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, path.as_str()).await;

    let app = make_app(handler);
    let cli = poem::test::TestClient::new(app);

    let url = format!("/publications/{}/download?inventory_id={}", work_id, inv_id);
    let resp = cli.get(url).send().await;
    resp.assert_status_is_ok();

    let headers = resp.0.headers();
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_default();
    assert_eq!(content_type, "application/epub+zip");

    let disposition = headers
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .unwrap_or_default();
    assert!(
        disposition.contains("attachment"),
        "expected attachment disposition, got {disposition:?}"
    );
    assert!(
        disposition.contains("pride.epub"),
        "expected filename in disposition, got {disposition:?}"
    );

    let body = resp.0.into_body().into_vec().await.expect("collect body");
    assert_eq!(body, b"PRIDE-AND-PREJUDICE");
}

#[tokio::test]
async fn download_route_serves_first_file_when_inventory_id_omitted() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let handler = make_handler(&test_db).await;
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "default.epub", b"DEFAULT-FILE");
    let work_id = seed_work(&repo, "Default Pick").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let _ = seed_inventory(&repo, edition_id, path.as_str()).await;

    let app = make_app(handler);
    let cli = poem::test::TestClient::new(app);

    let url = format!("/publications/{}/download", work_id);
    let resp = cli.get(url).send().await;
    resp.assert_status_is_ok();
    let body = resp.0.into_body().into_vec().await.expect("collect body");
    assert_eq!(body, b"DEFAULT-FILE");
}

#[tokio::test]
async fn download_route_returns_404_when_no_file_linked() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let handler = make_handler(&test_db).await;
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let work_id = seed_work(&repo, "Unlinked").await;

    let app = make_app(handler);
    let cli = poem::test::TestClient::new(app);

    let url = format!("/publications/{}/download", work_id);
    let resp = cli.get(url).send().await;
    resp.assert_status(poem::http::StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Mapper: acquisition links
// ---------------------------------------------------------------------------

#[tokio::test]
async fn mapper_emits_acquisition_link_for_edition_with_linked_file() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "downloadable.epub", b"epub");
    let work_id = seed_work(&repo, "Acquirable").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, path.as_str()).await;

    let agg = repo
        .get_work(&work_id.to_string())
        .await
        .expect("get_work")
        .expect("work present");

    let mapper = DefaultOpdsMapper::new("http://localhost/opds");
    let pub_ = mapper.work_to_publication(&agg);

    let acquisition = pub_
        .links
        .iter()
        .find(|l| {
            l.rel
                .iter()
                .any(|r| r == "http://opds-spec.org/acquisition")
        })
        .expect("acquisition link present");

    assert!(
        acquisition
            .href
            .contains(&format!("/publications/{}/download", work_id)),
        "acquisition href should hit download endpoint; got {}",
        acquisition.href
    );
    assert!(
        acquisition.href.contains(&format!("inventory_id={inv_id}")),
        "acquisition href should pin inventory_id; got {}",
        acquisition.href
    );
    assert_eq!(acquisition.type_, "application/epub+zip");
    let availability = acquisition
        .properties
        .as_ref()
        .and_then(|p| p.availability.as_ref())
        .expect("availability present");
    assert_eq!(availability.state, "available");
}

#[tokio::test]
async fn mapper_omits_acquisition_link_when_no_file_linked() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let work_id = seed_work(&repo, "Unlinked").await;

    let agg = repo
        .get_work(&work_id.to_string())
        .await
        .expect("get_work")
        .expect("work present");

    let mapper = DefaultOpdsMapper::new("http://localhost/opds");
    let pub_ = mapper.work_to_publication(&agg);

    let acquisition_count = pub_
        .links
        .iter()
        .filter(|l| {
            l.rel
                .iter()
                .any(|r| r == "http://opds-spec.org/acquisition")
        })
        .count();
    assert_eq!(
        acquisition_count, 0,
        "no acquisition link when no file is linked"
    );

    // The self link is still emitted so the publication record is
    // navigable; it must point at the record URL (not the
    // placeholder).
    let self_link = pub_
        .links
        .iter()
        .find(|l| l.is_self())
        .expect("self link present");
    assert_eq!(
        self_link.href,
        format!("http://localhost/opds/publications/{}", work_id)
    );
}

// ---------------------------------------------------------------------------
// Auth gating: the download route must require Basic Auth when configured.
// ---------------------------------------------------------------------------

fn make_app_with_auth(
    handler: SharedOpdsHandler,
    username: &str,
    password: &str,
) -> poem::endpoint::BoxEndpoint<'static, poem::Response> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHasher, SaltString},
    };
    use rand::{RngExt, SeedableRng};
    let seed = [0u8; 32];
    let mut rng = rand::rngs::StdRng::from_seed(seed);
    let mut salt_bytes = [0u8; 22];
    rng.fill(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).unwrap();
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash password")
        .to_string();

    let config = livtet_opds_server::AuthConfig {
        require_auth: true,
        username: username.to_string(),
        password_hash: hash,
    };

    livtet_opds_server::routes::make_routes(handler, Some(config))
}

#[tokio::test]
async fn download_route_rejects_unauthenticated_requests_with_401() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let handler = make_handler(&test_db).await;
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "secret.epub", b"SECRET-CONTENT");
    let work_id = seed_work(&repo, "Authenticated Book").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, path.as_str()).await;

    let app = make_app_with_auth(handler, "alice", "hunter2");
    let cli = poem::test::TestClient::new(app);

    let url = format!("/publications/{}/download?inventory_id={}", work_id, inv_id);
    let resp = cli.get(&url).send().await;
    resp.assert_status(poem::http::StatusCode::UNAUTHORIZED);

    let body = resp.0.into_body().into_vec().await.expect("collect body");
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.contains("opds-spec.org/auth/basic"),
        "expected OPDS Authentication Document in 401 body, got {body_str}"
    );
}

#[tokio::test]
async fn download_route_serves_file_with_valid_basic_auth() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let handler = make_handler(&test_db).await;
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);
    let tmp = camino_tempfile::tempdir().expect("tempdir");
    let path = write_file(tmp.path(), "open.epub", b"OPEN-CONTENT");
    let work_id = seed_work(&repo, "Open Book").await;
    let edition_id = seed_edition(&repo, work_id).await;
    let inv_id = seed_inventory(&repo, edition_id, path.as_str()).await;

    let app = make_app_with_auth(handler, "alice", "hunter2");
    let cli = poem::test::TestClient::new(app);

    let url = format!("/publications/{}/download?inventory_id={}", work_id, inv_id);

    let creds = base64_encode("alice:hunter2");
    let resp = cli
        .get(&url)
        .header("authorization", format!("Basic {creds}"))
        .send()
        .await;
    resp.assert_status_is_ok();

    let body = resp.0.into_body().into_vec().await.expect("collect body");
    assert_eq!(body, b"OPEN-CONTENT");
}

fn base64_encode(input: &str) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, input.as_bytes())
}
