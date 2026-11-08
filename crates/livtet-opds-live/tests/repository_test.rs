//! Integration tests for `LivtetOpdsRepository`.
//!
//! These tests exercise the concrete repository against a real SQLite
//! database (migrated via `livtet_core::test_db::TestDb`). They are the
//! primary safety net that the trait ↔ core CRUD translation is correct.

use std::sync::Arc;

use livtet_core::data::{
    Kind, TestDb,
    entities::{
        authors, digital_inventory, digital_inventory::Entity as EditionsInventory, editions,
    },
};
use livtet_opds_live::LivtetOpdsRepository;
use livtet_opds_server::{OpdsRepository, WorkQuery};
use livtet_search::SearchIndex;
use livtet_types::{DbId, KnownGenres, now_primitive};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait};

mod common;
use common::{CreateWorkInput, create_work};

/// Build a fresh Tantivy `SearchIndex` in a temp directory and `reindex`
/// it against the supplied database. The `Utf8TempDir` is returned so the
/// caller can keep the directory alive for the lifetime of the index.
///
/// Mirrors the helper in `crates/livtet-search/tests/integration.rs`
/// (see `fresh_index`). We need this because `LivtetOpdsRepository`
/// holds an `Arc<SearchIndex>` and the count / facet code paths now
/// go through Tantivy's `matching_work_ids`.
async fn fresh_search_index(
    db: &sea_orm::DatabaseConnection,
) -> (Arc<SearchIndex>, camino_tempfile::Utf8TempDir) {
    let dir = camino_tempfile::tempdir().expect("tempdir");
    let index = SearchIndex::open(dir.path()).expect("open index");
    index.reindex(db).await.expect("reindex");
    (Arc::new(index), dir)
}

// ---------------------------------------------------------------------------
// Live repository tests — exercise the SeaORM-backed implementation.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_works_returns_seeded_work_as_aggregate() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    // Seed a tag, a work, and an author binding using the public CRUD path.
    let tag_id = seed_tag(&test_db, "favorite").await;
    let author_id = seed_author(&test_db, "Octavia Butler").await;
    let work = create_work(
        repo.db(),
        CreateWorkInput {
            title: "Kindred".into(),
            description: Some("A novel about slavery and time travel.".into()),
            preferred_edition_id: None,
            work_authors: vec![(author_id, "author".into())],
            work_tags: vec![tag_id],
            work_genres: vec![],
            work_subjects: vec![],
            work_publishers: vec![],
        },
    )
    .await
    .expect("create work");

    // Add an edition so Tantivy picks up this work for search.
    let edition_id = DbId::new();
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work.work.id),
        created_at: Set(now_primitive()),
        updated_at: Set(Some(now_primitive())),
        ..Default::default()
    }
    .insert(repo.db())
    .await
    .expect("insert edition");

    // Reindex so the new work+edition is visible to Tantivy.
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let aggregates = repo
        .list_works(&WorkQuery::default())
        .await
        .expect("list_works");

    assert_eq!(aggregates.len(), 1, "expected exactly one work");
    let agg = &aggregates[0];
    assert_eq!(agg.id, work.work.id.to_string());
    assert_eq!(agg.title, "Kindred");
    assert_eq!(agg.authors.len(), 1);
    assert_eq!(agg.authors[0].name, "Octavia Butler");
    assert_eq!(agg.tags, vec!["favorite".to_string()]);
    assert_eq!(agg.editions.len(), 1, "should have one edition");
}

#[tokio::test]
async fn count_works_matches_list_size() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    for title in ["Alpha", "Bravo", "Charlie"] {
        let work = create_work(
            repo.db(),
            CreateWorkInput {
                title: title.into(),
                description: None,
                preferred_edition_id: None,
                work_authors: vec![],
                work_tags: vec![],
                work_genres: vec![],
                work_subjects: vec![],
                work_publishers: vec![],
            },
        )
        .await
        .expect("create work");

        // Add an edition so Tantivy picks up this work.
        editions::ActiveModel {
            id: Set(DbId::new()),
            work_id: Set(work.work.id),
            created_at: Set(now_primitive()),
            updated_at: Set(Some(now_primitive())),
            ..Default::default()
        }
        .insert(repo.db())
        .await
        .expect("insert edition");
    }

    // Reindex so the new works are visible to Tantivy's matching_work_ids.
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let count = repo
        .count_works(&WorkQuery::default())
        .await
        .expect("count");
    let list = repo.list_works(&WorkQuery::default()).await.expect("list");
    assert_eq!(count as usize, list.len());
    assert_eq!(count, 3);
}

#[tokio::test]
async fn get_work_returns_none_for_missing_id() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    // A freshly minted ULID that no row uses.
    let missing = DbId::new();
    let result = repo.get_work(&missing.to_string()).await.expect("get_work");
    assert!(
        result.is_none(),
        "expected None for missing id, got {result:?}"
    );
}

#[tokio::test]
async fn get_work_returns_none_for_garbage_id() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    // "not-a-ulid" is not a valid ULID — `get_work` must treat that as a
    // 404, not a 500, because remote clients will inevitably send junk.
    let result = repo.get_work("not-a-ulid").await.expect("get_work");
    assert!(result.is_none(), "garbage id should return None");
}

#[tokio::test]
async fn get_work_round_trip() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let created = create_work(
        repo.db(),
        CreateWorkInput {
            title: "Parable of the Sower".into(),
            description: None,
            preferred_edition_id: None,
            work_authors: vec![],
            work_tags: vec![],
            work_genres: vec![],
            work_subjects: vec![],
            work_publishers: vec![],
        },
    )
    .await
    .expect("create work");

    let fetched = repo
        .get_work(&created.work.id.to_string())
        .await
        .expect("get_work")
        .expect("present");
    assert_eq!(fetched.id, created.work.id.to_string());
    assert_eq!(fetched.title, "Parable of the Sower");
}

#[tokio::test]
async fn list_authors_returns_sorted() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    // Insert in non-alphabetical order; list_authors must sort by name asc.
    for name in ["Charlie", "Alice", "Bob"] {
        seed_author(&test_db, name).await;
    }

    let authors = repo.list_authors().await.expect("list_authors");
    let names: Vec<&str> = authors.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["Alice", "Bob", "Charlie"]);
    for a in &authors {
        assert_eq!(a.role, "author");
        assert!(!a.id.is_empty());
    }
}

#[tokio::test]
async fn list_genres_and_tags_round_trip() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    seed_genre(&test_db, "Sci-Fi").await;
    seed_tag(&test_db, "library").await;

    let mut genres = repo.list_genres().await.expect("list_genres");
    genres.sort();
    let mut expected: Vec<String> = KnownGenres::all()
        .into_iter()
        .map(|g| g.name().to_string())
        .collect();
    expected.push("Sci-Fi".to_string());
    expected.sort();
    assert_eq!(genres, expected);

    let tags = repo.list_tags().await.expect("list_tags");
    assert_eq!(tags, vec!["library".to_string()]);
}

#[tokio::test]
async fn inventory_paths_surface_on_edition_aggregate() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let created = create_work(
        repo.db(),
        CreateWorkInput {
            title: "Piranesi".into(),
            description: None,
            preferred_edition_id: None,
            work_authors: vec![],
            work_tags: vec![],
            work_genres: vec![],
            work_subjects: vec![],
            work_publishers: vec![],
        },
    )
    .await
    .expect("create work");

    // `create_work` no longer creates a default edition, so we create one
    // explicitly so we can attach inventory and verify the batch-load
    // stitches the path on.
    let edition = editions::ActiveModel {
        id: Set(DbId::new()),
        work_id: Set(created.work.id),
        created_at: Set(now_primitive()),
        updated_at: Set(Some(now_primitive())),
        ..Default::default()
    }
    .insert(repo.db())
    .await
    .expect("insert edition");

    let inv = digital_inventory::ActiveModel {
        id: Set(DbId::new()),
        edition_id: Set(edition.id),
        file_path: Set(Some("/library/piranesi.epub".into())),
        cover_path: Set(Some("/covers/piranesi.jpg".into())),
        file_hash: Set(None),
        file_size_bytes: Set(None),
        notes: Set(None),
        // `added_at` and `updated_at` keep their `Default` value rather
        // than being re-assigned in place; this matches the clippy
        // `field_reassign_with_default` style.
        ..Default::default()
    };
    EditionsInventory::insert(inv)
        .exec(repo.db())
        .await
        .expect("insert inventory");

    let agg = repo
        .get_work(&created.work.id.to_string())
        .await
        .expect("get_work")
        .expect("present");

    assert_eq!(agg.editions.len(), 1);
    let edition = &agg.editions[0];
    assert_eq!(edition.cover_path.as_deref(), Some("/covers/piranesi.jpg"));
    assert_eq!(edition.file_path.as_deref(), Some("/library/piranesi.epub"));
}

// ---------------------------------------------------------------------------
// Seeding helpers
// ---------------------------------------------------------------------------

async fn seed_author(test_db: &TestDb, name: &str) -> DbId {
    use sea_orm::ActiveModelTrait;
    let db = test_db.state().db_conn();
    let id = DbId::new();
    let active = authors::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        family: Set(None),
        given: Set(None),
    };
    active.insert(&db).await.expect("insert author");
    id
}

async fn seed_genre(test_db: &TestDb, name: &str) -> DbId {
    use livtet_core::data::entities::genres;
    use sea_orm::ActiveModelTrait;
    let db = test_db.state().db_conn();
    let id = DbId::new();
    let active = genres::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        created_at: Set(time::PrimitiveDateTime::new(
            time::OffsetDateTime::now_utc().date(),
            time::OffsetDateTime::now_utc().time(),
        )),
    };
    active.insert(&db).await.expect("insert genre");
    id
}

async fn seed_tag(test_db: &TestDb, name: &str) -> DbId {
    use livtet_core::data::entities::tags;
    use sea_orm::ActiveModelTrait;
    let db = test_db.state().db_conn();
    let id = DbId::new();
    let active = tags::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        created_at: Set(time::PrimitiveDateTime::new(
            time::OffsetDateTime::now_utc().date(),
            time::OffsetDateTime::now_utc().time(),
        )),
        updated_at: Set(Some(time::PrimitiveDateTime::new(
            time::OffsetDateTime::now_utc().date(),
            time::OffsetDateTime::now_utc().time(),
        ))),
    };
    active.insert(&db).await.expect("insert tag");
    id
}
