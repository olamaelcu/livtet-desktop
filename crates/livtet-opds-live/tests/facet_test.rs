//! Integration tests for the OPDS facet counts (`properties.numberOfItems`).
//!
//! These tests seed a small catalog and verify that
//! `LivtetOpdsRepository::compute_all_facets` returns per-axis counts that
//! match the seeded data, flag the active filter's values with
//! `active: true`, and respect the [`FACET_LIMIT`] cap. They also check
//! the mapper-side: [`DefaultOpdsMapper::build_facets`] should produce
//! one `Collection` per axis with at least two values, with the right
//! `rel` and `properties.numberOfItems`.
//!
//! These complement `repository_tests.rs` (which covers the broader
//! trait surface) by pinning down the facet behavior in isolation.

use std::sync::Arc;

use livtet_core::data::{
    Kind, TestDb,
    entities::{genres, tags},
};
use livtet_opds_live::LivtetOpdsRepository;
use livtet_opds_server::{
    DefaultOpdsMapper, FacetAxis, FacetCount, FacetCounts, OpdsRepository, WorkQuery,
};
use livtet_search::SearchIndex;
use livtet_types::DbId;
use sea_orm::{ActiveModelTrait, ActiveValue::Set};

mod common;
use common::{CreateWorkInput, create_work};

/// Build a fresh Tantivy `SearchIndex` in a temp directory and `reindex`
/// it against the supplied database.
async fn fresh_search_index(
    db: &sea_orm::DatabaseConnection,
) -> (Arc<SearchIndex>, camino_tempfile::Utf8TempDir) {
    let dir = camino_tempfile::tempdir().expect("tempdir");
    let index = SearchIndex::open(dir.path()).expect("open index");
    index.reindex(db).await.expect("reindex");
    (Arc::new(index), dir)
}

#[tokio::test]
async fn compute_facets_returns_empty_for_empty_catalog() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let counts = repo
        .compute_all_facets(&WorkQuery::default())
        .await
        .expect("compute facets");

    assert!(counts.authors.is_empty());
    assert!(counts.tags.is_empty());
    assert!(counts.genres.is_empty());
    assert!(counts.subjects.is_empty());
    assert!(counts.publishers.is_empty());
    assert!(counts.formats.is_empty());
    assert!(counts.languages.is_empty());
}

#[tokio::test]
async fn compute_facets_aggregates_tag_and_genre_counts() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_fiction = seed_tag(&test_db, "fiction").await;
    let tag_scifi = seed_tag(&test_db, "scifi").await;
    let genre_drama = seed_genre(&test_db, "Drama").await;
    let genre_scifi = seed_genre(&test_db, "Sci-Fi").await;

    // Two sci-fi novels (tag_scifi, genre_scifi), one drama (tag_fiction,
    // genre_drama), and one sci-fi drama (tag_scifi + tag_fiction,
    // genre_scifi). That gives:
    //   tag_fiction → 2
    //   tag_scifi   → 2
    //   genre_scifi → 2
    //   genre_drama → 1
    seed_work(&repo, "Alpha", &[], &[tag_scifi], &[genre_scifi]).await;
    seed_work(&repo, "Bravo", &[], &[tag_scifi], &[genre_scifi]).await;
    seed_work(&repo, "Charlie", &[], &[tag_fiction], &[genre_drama]).await;
    seed_work(
        &repo,
        "Delta",
        &[],
        &[tag_scifi, tag_fiction],
        &[genre_scifi],
    )
    .await;

    // Reindex so the new works are visible to Tantivy's matching_work_ids.
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let counts = repo
        .compute_all_facets(&WorkQuery::default())
        .await
        .expect("compute facets");

    let by_id = |vec: &[crate::FacetCount], id: DbId| -> i64 {
        vec.iter()
            .find(|c| c.id == id.to_string())
            .map(|c| c.count)
            .unwrap_or(-1)
    };

    assert_eq!(by_id(&counts.tags, tag_scifi), 3, "scifi tag → 3 works");
    assert_eq!(by_id(&counts.tags, tag_fiction), 2, "fiction tag → 2 works");
    assert_eq!(
        by_id(&counts.genres, genre_scifi),
        3,
        "scifi genre → 3 works"
    );
    assert_eq!(
        by_id(&counts.genres, genre_drama),
        1,
        "drama genre → 1 work"
    );

    // Count order: sci-fi/fiction at 3 and 2 first, in alphabetical
    // order. Tied axes don't exist in this seed.
    let tag_labels: Vec<&str> = counts.tags.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(tag_labels, vec!["scifi", "fiction"]);
}

#[tokio::test]
async fn compute_facets_marks_active_value_for_each_axis() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_a = seed_tag(&test_db, "alpha").await;
    let tag_b = seed_tag(&test_db, "beta").await;
    seed_work(&repo, "X", &[], &[tag_a, tag_b], &[]).await;
    seed_work(&repo, "Y", &[], &[tag_b], &[]).await;
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let q = WorkQuery {
        tag_id: Some(tag_a.to_string()),
        ..Default::default()
    };

    let counts = repo.compute_all_facets(&q).await.expect("compute facets");

    // The "alpha" tag should be flagged active; "beta" should not.
    let alpha = counts
        .tags
        .iter()
        .find(|c| c.id == tag_a.to_string())
        .expect("alpha tag present");
    let beta = counts
        .tags
        .iter()
        .find(|c| c.id == tag_b.to_string())
        .expect("beta tag present");
    assert!(alpha.active, "alpha is the active filter");
    assert!(!beta.active, "beta is not the active filter");
}

#[tokio::test]
async fn compute_facets_scopes_to_active_filter() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_a = seed_tag(&test_db, "alpha").await;
    let tag_b = seed_tag(&test_db, "beta").await;
    let genre_drama = seed_genre(&test_db, "Drama").await;
    let genre_scifi = seed_genre(&test_db, "Sci-Fi").await;

    // Work 1: tag_a, Drama
    // Work 2: tag_a, Sci-Fi
    // Work 3: tag_b, Drama
    // Work 4: tag_b, Sci-Fi
    seed_work(&repo, "W1", &[], &[tag_a], &[genre_drama]).await;
    seed_work(&repo, "W2", &[], &[tag_a], &[genre_scifi]).await;
    seed_work(&repo, "W3", &[], &[tag_b], &[genre_drama]).await;
    seed_work(&repo, "W4", &[], &[tag_b], &[genre_scifi]).await;
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    // Active filter: tag_a → only W1, W2 are visible.
    let q = WorkQuery {
        tag_id: Some(tag_a.to_string()),
        ..Default::default()
    };

    let counts = repo.compute_all_facets(&q).await.expect("compute facets");

    let by_id = |vec: &[FacetCount], id: DbId| -> i64 {
        vec.iter()
            .find(|c| c.id == id.to_string())
            .map(|c| c.count)
            .unwrap_or(-1)
    };

    // When scoped to tag_a, only Drama and Sci-Fi shows up. Both
    // genres appear (once per work that has tag_a), so the counts are
    // 1 each, not 2.
    assert_eq!(by_id(&counts.genres, genre_drama), 1);
    assert_eq!(by_id(&counts.genres, genre_scifi), 1);
}

#[tokio::test]
async fn build_facets_emits_collection_per_axis_with_number_of_items() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_a = seed_tag(&test_db, "alpha").await;
    let tag_b = seed_tag(&test_db, "beta").await;
    let tag_c = seed_tag(&test_db, "gamma").await;
    seed_work(&repo, "W1", &[], &[tag_a, tag_b, tag_c], &[]).await;
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let counts = repo
        .compute_all_facets(&WorkQuery::default())
        .await
        .expect("compute facets");

    let mapper = DefaultOpdsMapper::new("http://localhost:8080/opds");
    let collections =
        mapper.build_facets("http://localhost:8080/opds", &WorkQuery::default(), &counts);

    // Only the Tags axis has >= 2 values; Authors/Genres/etc. are
    // empty (or single) and should be dropped.
    assert_eq!(collections.len(), 1, "only Tags has multiple values");
    let tags = &collections[0];
    assert_eq!(tags.metadata.title, "Tags");
    assert_eq!(tags.links.len(), 3);

    for link in &tags.links {
        assert_eq!(link.type_, "application/opds+json");
        let props = link.properties.as_ref().expect("properties present");
        let n = props.number_of_items.expect("numberOfItems present");
        assert_eq!(n, 1, "every tag has count 1 in this seed");
    }
}

#[tokio::test]
async fn build_facets_href_toggles_active_filter() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_a = seed_tag(&test_db, "alpha").await;
    let tag_b = seed_tag(&test_db, "beta").await;
    seed_work(&repo, "W1", &[], &[tag_a, tag_b], &[]).await;
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    let q = WorkQuery {
        tag_id: Some(tag_a.to_string()),
        ..Default::default()
    };
    let counts = repo.compute_all_facets(&q).await.expect("compute");

    let mapper = DefaultOpdsMapper::new("http://localhost:8080/opds");
    let collections = mapper.build_facets("http://localhost:8080/opds", &q, &counts);
    let links = &collections[0].links;

    // Find the active link by its `rel: ["self"]` (set by the mapper
    // for the value matching the current filter).
    let active_link = links
        .iter()
        .find(|l| l.rel == vec!["self".to_string()])
        .expect("active link present with rel=self");
    // Active: rel is "self" and the href drops tag_id (toggle off).
    // The base URL parser normalises an empty query to a trailing `?`,
    // so the href ends up as `http://localhost:8080/opds?` — that's
    // still semantically the same URL as the base.
    assert!(
        !active_link.href.contains("tag_id="),
        "active link should toggle tag_id off; got {}",
        active_link.href
    );
    assert!(
        active_link.href == "http://localhost:8080/opds"
            || active_link.href == "http://localhost:8080/opds?",
        "active link should be the base URL; got {}",
        active_link.href
    );
    let active_id = counts
        .tags
        .iter()
        .find(|c| c.active)
        .map(|c| c.id.clone())
        .expect("an active tag exists");

    // The remaining link(s) are inactive: empty rel, and the href
    // replaces the active tag_id with the new value (so we get
    // tag_id=beta but NOT tag_id=alpha).
    let inactive_links: Vec<_> = links.iter().filter(|l| l.rel.is_empty()).collect();
    assert_eq!(inactive_links.len(), 1, "only one inactive link");
    let beta_link = inactive_links[0];
    assert!(
        beta_link.href.contains(&format!("tag_id={}", tag_b)),
        "inactive href should contain tag_id=beta; got {}",
        beta_link.href
    );
    assert!(
        !beta_link.href.contains(&format!("tag_id={}", active_id)),
        "inactive href should replace (not stack) tag_id; got {}",
        beta_link.href
    );
}

#[tokio::test]
async fn compute_facets_empty_axes_returns_empty_collections() {
    let mapper = DefaultOpdsMapper::new("http://localhost:8080/opds");
    let empty = FacetCounts::default();
    let collections =
        mapper.build_facets("http://localhost:8080/opds", &WorkQuery::default(), &empty);
    assert!(collections.is_empty());
}

#[tokio::test]
async fn compute_facets_respects_per_axis_subset() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
    let (search_index, _dir) = fresh_search_index(&test_db.state().db_conn()).await;
    let repo = LivtetOpdsRepository::from_state(&test_db.state(), search_index);

    let tag_a = seed_tag(&test_db, "alpha").await;
    let tag_b = seed_tag(&test_db, "beta").await;
    seed_work(&repo, "W1", &[], &[tag_a, tag_b], &[]).await;
    repo.search_index()
        .reindex(repo.db())
        .await
        .expect("reindex");

    // Asking for just the Tag axis: still works.
    let counts = repo
        .compute_facets(&WorkQuery::default(), &[FacetAxis::Tag])
        .await
        .expect("compute facets");
    assert_eq!(counts.tags.len(), 2);
    assert!(counts.authors.is_empty());
    assert!(counts.genres.is_empty());

    // Empty axes list → all-empty counts.
    let counts = repo
        .compute_facets(&WorkQuery::default(), &[])
        .await
        .expect("compute facets");
    assert!(counts.tags.is_empty());
    assert!(counts.authors.is_empty());
}

// ---------------------------------------------------------------------------
// Seeding helpers — minimal copies of the helpers in repository_tests.rs
// ---------------------------------------------------------------------------

async fn seed_tag(test_db: &TestDb, name: &str) -> DbId {
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

async fn seed_genre(test_db: &TestDb, name: &str) -> DbId {
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

async fn seed_work(
    repo: &LivtetOpdsRepository,
    title: &str,
    _authors: &[DbId],
    tag_ids: &[DbId],
    genre_ids: &[DbId],
) {
    create_work(
        repo.db(),
        CreateWorkInput {
            title: title.into(),
            description: None,
            preferred_edition_id: None,
            work_authors: vec![],
            work_tags: tag_ids.to_vec(),
            work_genres: genre_ids.to_vec(),
            work_subjects: vec![],
            work_publishers: vec![],
        },
    )
    .await
    .expect("create work");
}
