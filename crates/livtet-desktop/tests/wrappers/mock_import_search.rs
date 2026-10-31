#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Mock-app tests for the `search`, `bulk` (tag/export) and `import` wrappers.
//!
//! Every test builds a real `AppState` from [`mock_state_for_bulk`], points the
//! book/cover directories at a private temp dir, and calls the
//! `#[tauri::command]` function the way IPC does: its `State<AppState>`
//! argument is `app.state::<AppState>()`.
//!
//! Index-backed assertions open a real Tantivy index in a temp dir and fill it
//! from the `TestDb` with `SearchIndex::reindex`, so the search answers are
//! produced by the same code the app runs.

use std::{collections::HashMap, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};
use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
    edition_tags, editions, formats, genres, identifiers, languages, publishers, subjects, tags,
    works,
};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    Set,
};
use livtet_core::data::{Kind, TestDb};
use livtet_core::search::SearchIndex;
use livtet_desktop_lib::commands::bulk::{
    add_edition_tags, export_editions_csv, matching_edition_ids, remove_edition_tags,
};
use livtet_desktop_lib::commands::catalog::FileStatus;
use livtet_desktop_lib::commands::import::{
    VirtualAuthor, VirtualBookInput, create_virtual_book, relink_edition_file, sync_shelf_placement,
};
use livtet_desktop_lib::commands::search::{
    EditionFilters, FilterOption, filter_options, search_editions, search_typeahead,
};
use livtet_desktop_lib::{AppState, SearchIndexError, mock_state_for_bulk};
use livtet_types::{DbId, KnownFormats, now_primitive};
use sha2::Digest;
use tauri::Manager;

// ---- fixtures --------------------------------------------------------------

fn utf8<P: AsRef<std::ffi::OsStr>>(path: P) -> Utf8PathBuf {
    Utf8PathBuf::from_os_string(path.as_ref().to_os_string()).expect("utf8 temp path")
}

/// Mock `AppState` whose book/cover directories live under `root`.
async fn state_at(test_db: &TestDb, root: &Utf8Path) -> AppState {
    let mut state = mock_state_for_bulk(test_db).await;
    state.books_dir = root.join("books");
    state.covers_dir = root.join("covers");
    state
}

/// Open (and optionally fill from `db`) a real index and attach it to `state`.
async fn attach_index(state: &mut AppState, index_dir: &Utf8Path, db: Option<&DatabaseConnection>) {
    let index = SearchIndex::open(index_dir).expect("search index");
    if let Some(db) = db {
        index.reindex(db).await.expect("reindex");
    }
    *state.search_index.write().await = Some(Arc::new(index));
}

async fn seed_work(db: &DatabaseConnection, title: &str) -> DbId {
    let id = DbId::new();
    works::ActiveModel {
        id: Set(id),
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
    .expect("work");
    id
}

async fn seed_edition(
    db: &DatabaseConnection,
    work_id: DbId,
    title: Option<&str>,
    format_id: Option<DbId>,
    language_id: Option<DbId>,
    published_date: Option<time::Date>,
) -> DbId {
    let id = DbId::new();
    editions::ActiveModel {
        id: Set(id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(title.map(str::to_string)),
        published_date: Set(published_date),
        format_id: Set(format_id),
        language_id: Set(language_id),
        notes: Set(None),
        description: Set(None),
        format_metadata: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("edition");
    id
}

/// `file_path` is `None` for a virtual book (an inventory row without a file).
async fn seed_inventory(db: &DatabaseConnection, edition_id: DbId, file_path: Option<String>) {
    digital_inventory::ActiveModel {
        id: Set(DbId::new()),
        edition_id: Set(edition_id),
        file_path: Set(file_path),
        cover_path: Set(None),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(None),
        file_size_bytes: Set(None),
        file_format: Set(Some("epub".to_string())),
        notes: Set(None),
        added_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("inventory");
}

/// Work + edition + one author, the minimum a searchable book needs.
async fn seed_book(
    db: &DatabaseConnection,
    title: &str,
    author: &str,
    description: &str,
) -> (DbId, DbId) {
    let work_id = seed_work(db, title).await;
    let edition_id = DbId::new();
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some(title.to_string())),
        published_date: Set(None),
        format_id: Set(None),
        language_id: Set(None),
        notes: Set(None),
        description: Set(Some(description.to_string())),
        format_metadata: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("edition");

    let author_id = DbId::new();
    authors::ActiveModel {
        id: Set(author_id),
        name: Set(author.to_string()),
    }
    .insert(db)
    .await
    .expect("author");
    edition_authors::ActiveModel {
        edition_id: Set(edition_id),
        author_id: Set(author_id),
        role: Set("aut".to_string()),
    }
    .insert(db)
    .await
    .expect("edition author");
    (work_id, edition_id)
}

async fn seed_tag(db: &DatabaseConnection, name: &str) -> DbId {
    let id = DbId::new();
    tags::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("tag");
    id
}

async fn inventory_of(db: &DatabaseConnection, edition_id: DbId) -> digital_inventory::Model {
    digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await
        .expect("inventory query")
        .expect("inventory row")
}

fn find(options: &[FilterOption], id: DbId) -> &FilterOption {
    options
        .iter()
        .find(|option| option.id == id)
        .unwrap_or_else(|| panic!("no filter option for {id}"))
}

fn sorted_ids(ids: &[DbId]) -> Vec<String> {
    let mut out: Vec<String> = ids.iter().map(ToString::to_string).collect();
    out.sort();
    out
}

fn virtual_input(title: &str) -> VirtualBookInput {
    VirtualBookInput {
        title: title.to_string(),
        authors: Vec::new(),
        publisher: None,
        language_id: None,
        format_id: None,
        identifiers: Vec::new(),
        description: None,
        notes: None,
        cover_image_url: None,
    }
}

// ---- search_editions -------------------------------------------------------

#[tokio::test]
async fn search_editions_recomputes_has_file_from_the_inventory_rows() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let (_, on_disk) =
        seed_book(&db, "Foundation", "Isaac Asimov", "a galactic empire falls").await;
    seed_inventory(&db, on_disk, Some("/books/foundation.epub".into())).await;
    let (_, virtual_book) =
        seed_book(&db, "Dune", "Frank Herbert", "a galactic spice empire").await;
    seed_inventory(&db, virtual_book, None).await;

    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), Some(&db)).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let response = search_editions(
        Some("galactic".into()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("search");

    assert_eq!(response.total, 2, "both editions match `galactic`");
    assert_eq!(response.hits.len(), 2);
    let has_file: HashMap<String, bool> = response
        .hits
        .iter()
        .map(|hit| (hit.edition_id.clone().expect("edition hit"), hit.has_file))
        .collect();
    assert_eq!(
        has_file.get(&on_disk.to_string()).copied(),
        Some(true),
        "an inventory row with a path means the file is on disk"
    );
    assert_eq!(
        has_file.get(&virtual_book.to_string()).copied(),
        Some(false),
        "a NULL file_path stays virtual even though the index saw an inventory row"
    );
}

#[tokio::test]
async fn search_editions_honours_offset_and_limit_but_counts_every_match() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    seed_book(&db, "Foundation", "Isaac Asimov", "a galactic empire").await;
    seed_book(&db, "Dune", "Frank Herbert", "a galactic empire").await;

    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), Some(&db)).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let response = search_editions(
        Some("galactic".into()),
        None,
        Some(1),
        Some(1),
        app.state::<AppState>(),
    )
    .await
    .expect("search");

    assert_eq!(response.hits.len(), 1, "limit caps the page");
    assert_eq!(response.total, 2, "total ignores the page window");
}

#[tokio::test]
async fn search_editions_answers_an_empty_index_with_an_empty_response() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    seed_book(&db, "Foundation", "Isaac Asimov", "a galactic empire").await;

    let mut state = state_at(&test_db, &root).await;
    // Opened but never filled: the wrapper must answer, not error.
    attach_index(&mut state, &root.join("index"), None).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let response = search_editions(
        Some("Foundation".into()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect("an empty index is not a failure");
    assert!(response.hits.is_empty());
    assert_eq!(response.total, 0);

    let unfiltered = search_editions(None, None, None, None, app.state::<AppState>())
        .await
        .expect("match-all over an empty index");
    assert!(unfiltered.hits.is_empty());
    assert_eq!(unfiltered.total, 0);
}

#[tokio::test]
async fn search_editions_without_an_index_reports_unavailable() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());
    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let error = search_editions(
        Some("anything".into()),
        None,
        None,
        None,
        app.state::<AppState>(),
    )
    .await
    .expect_err("no index must fail");

    assert!(
        matches!(&error, SearchIndexError::Unavailable(what) if what == "search_editions"),
        "unexpected error: {error:?}"
    );
}

// ---- search_typeahead ------------------------------------------------------

#[tokio::test]
async fn search_typeahead_returns_index_matches_within_the_limit() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let (_, foundation) =
        seed_book(&db, "Foundation", "Isaac Asimov", "a galactic empire falls").await;
    seed_book(&db, "Second Foundation", "Isaac Asimov", "the other empire").await;

    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), Some(&db)).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let results = search_typeahead("Foundation".into(), Some(1), app.state::<AppState>())
        .await
        .expect("typeahead");

    assert_eq!(results.len(), 1, "limit caps the suggestion list");
    assert!(
        results
            .iter()
            .any(|hit| hit.edition_id.as_deref() == Some(foundation.to_string().as_str())),
        "first hit is one of the seeded editions: {:?}",
        results
    );

    let all = search_typeahead("Foundation".into(), None, app.state::<AppState>())
        .await
        .expect("typeahead without a limit");
    assert_eq!(all.len(), 2, "both Foundation editions match");
}

#[tokio::test]
async fn search_typeahead_blank_input_short_circuits_before_the_index() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());
    let app = tauri::test::mock_app();
    // No index attached: only the blank-input guard can keep this from failing.
    app.manage(state_at(&test_db, &root).await);

    let results = search_typeahead("   ".into(), None, app.state::<AppState>())
        .await
        .expect("blank input never consults the index");
    assert!(results.is_empty());

    let error = search_typeahead("Dune".into(), None, app.state::<AppState>())
        .await
        .expect_err("a real query needs the index");
    assert!(
        matches!(&error, SearchIndexError::Unavailable(what) if what == "search_typeahead"),
        "unexpected error: {error:?}"
    );
}

// ---- filter_options --------------------------------------------------------

#[tokio::test]
async fn filter_options_lists_every_axis_sorted_and_decorated() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let now = now_primitive();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let tag_id = seed_tag(&db, "classic").await;

    let genre_id = DbId::new();
    genres::ActiveModel {
        id: Set(genre_id),
        name: Set("space opera".to_string()),
        created_at: Set(now),
    }
    .insert(&db)
    .await
    .expect("genre");

    let subject_id = DbId::new();
    subjects::ActiveModel {
        id: Set(subject_id),
        name: Set("psychohistory".to_string()),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("subject");

    let publisher_id = DbId::new();
    publishers::ActiveModel {
        id: Set(publisher_id),
        name: Set("Gnome Press".to_string()),
        website: Set(None),
        logo_url: Set(Some("https://example.com/gnome.png".to_string())),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("publisher");

    let author_id = DbId::new();
    authors::ActiveModel {
        id: Set(author_id),
        name: Set("Isaac Asimov".to_string()),
    }
    .insert(&db)
    .await
    .expect("author");

    let format_id = DbId::new();
    formats::ActiveModel {
        id: Set(format_id),
        name: Set("manga".to_string()),
        metadata_schema: Set(serde_json::Value::Null),
        progress_unit: Set(None),
    }
    .insert(&db)
    .await
    .expect("format");

    let language_id = DbId::new();
    languages::ActiveModel {
        id: Set(language_id),
        name: Set("frenchish".to_string()),
        code: Set("fr-x".to_string()),
        flag_emoji: Set(Some("🇫🇷".to_string())),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("language");

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let options = filter_options(app.state::<AppState>())
        .await
        .expect("filter options");

    assert_eq!(find(&options.tags, tag_id).label, "classic");
    assert_eq!(find(&options.genres, genre_id).label, "space opera");
    assert_eq!(find(&options.subjects, subject_id).label, "psychohistory");
    assert_eq!(find(&options.authors, author_id).label, "Isaac Asimov");

    let format = find(&options.formats, format_id);
    assert_eq!(format.label, "manga");
    assert_eq!(
        (format.flag_emoji.as_deref(), format.logo_url.as_deref()),
        (None, None)
    );

    let language = find(&options.languages, language_id);
    assert_eq!(language.label, "frenchish");
    assert_eq!(language.flag_emoji.as_deref(), Some("🇫🇷"));
    assert_eq!(language.logo_url, None, "languages carry no logo");

    let publisher = find(&options.publishers, publisher_id);
    assert_eq!(publisher.label, "Gnome Press");
    assert_eq!(
        publisher.logo_url.as_deref(),
        Some("https://example.com/gnome.png")
    );
    assert_eq!(publisher.flag_emoji, None, "publishers carry no flag");

    // Seeded catalog defaults are still offered.
    let epub: DbId = KnownFormats::Epub.into();
    assert!(
        options.formats.iter().any(|option| option.id == epub),
        "the seeded EPUB format is listed"
    );

    for axis in [
        &options.formats,
        &options.languages,
        &options.tags,
        &options.genres,
        &options.subjects,
        &options.publishers,
        &options.authors,
    ] {
        assert!(
            axis.windows(2).all(|pair| pair[0].label <= pair[1].label),
            "axis is ordered by name: {axis:?}"
        );
    }
}

// ---- add_edition_tags / remove_edition_tags -------------------------------

#[tokio::test]
async fn add_edition_tags_creates_the_tag_once_and_links_every_edition() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let work_id = seed_work(&db, "Foundation").await;
    let first = seed_edition(&db, work_id, Some("Foundation"), None, None, None).await;
    let second = seed_edition(
        &db,
        work_id,
        Some("Foundation and Empire"),
        None,
        None,
        None,
    )
    .await;

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let outcome = add_edition_tags(
        vec![first, second],
        "  classic  ".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("add tag");

    assert_eq!(outcome.changed, 2, "both editions gained the link");
    assert_eq!(outcome.tag.label, "classic", "the name is trimmed");

    let stored = tags::Entity::find()
        .filter(tags::Column::Name.eq("classic"))
        .one(&db)
        .await
        .expect("tag query")
        .expect("tag row");
    assert_eq!(stored.id, outcome.tag.id);
    assert_eq!(
        tags::Entity::find().count(&db).await.expect("tag count"),
        1,
        "the tag is created once"
    );
    assert_eq!(
        edition_tags::Entity::find()
            .count(&db)
            .await
            .expect("link count"),
        2
    );

    // Re-adding the same tag is a no-op, not a duplicate row.
    let again = add_edition_tags(
        vec![first, second],
        "classic".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("add tag again");
    assert_eq!(again.changed, 0);
    assert_eq!(
        edition_tags::Entity::find()
            .count(&db)
            .await
            .expect("link count"),
        2
    );

    let error = add_edition_tags(vec![first], "   ".to_string(), app.state::<AppState>())
        .await
        .expect_err("an empty tag name is rejected");
    assert_eq!(error.code, "invalid_input");
}

#[tokio::test]
async fn remove_edition_tags_detaches_only_the_named_tag() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let work_id = seed_work(&db, "Foundation").await;
    let first = seed_edition(&db, work_id, Some("Foundation"), None, None, None).await;
    let second = seed_edition(
        &db,
        work_id,
        Some("Foundation and Empire"),
        None,
        None,
        None,
    )
    .await;

    let classic = seed_tag(&db, "classic").await;
    let owned = seed_tag(&db, "owned").await;
    for edition_id in [first, second] {
        edition_tags::ActiveModel {
            edition_id: Set(edition_id),
            tag_id: Set(classic),
        }
        .insert(&db)
        .await
        .expect("classic link");
    }
    edition_tags::ActiveModel {
        edition_id: Set(first),
        tag_id: Set(owned),
    }
    .insert(&db)
    .await
    .expect("owned link");

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let outcome = remove_edition_tags(vec![first, second], classic, app.state::<AppState>())
        .await
        .expect("remove tag");
    assert_eq!(outcome.changed, 2);
    assert_eq!(outcome.tag.label, "classic");

    let remaining: Vec<(String, String)> = edition_tags::Entity::find()
        .all(&db)
        .await
        .expect("links")
        .into_iter()
        .map(|link| (link.edition_id.to_string(), link.tag_id.to_string()))
        .collect();
    assert_eq!(remaining, vec![(first.to_string(), owned.to_string())]);
    assert!(
        edition_tags::Entity::find()
            .filter(edition_tags::Column::TagId.eq(classic))
            .one(&db)
            .await
            .expect("classic links")
            .is_none(),
        "the named tag is gone from both editions"
    );

    let again = remove_edition_tags(vec![first], classic, app.state::<AppState>())
        .await
        .expect("re-removing is a no-op");
    assert_eq!(again.changed, 0);

    let error = remove_edition_tags(vec![first], DbId::new(), app.state::<AppState>())
        .await
        .expect_err("an unknown tag id is reported");
    assert_eq!(error.code, "not_found");
}

// ---- matching_edition_ids --------------------------------------------------

#[tokio::test]
async fn matching_edition_ids_lists_every_edition_of_the_matching_works() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let foundation_work = seed_work(&db, "Foundation").await;
    let first = seed_edition(&db, foundation_work, Some("Foundation"), None, None, None).await;
    let second = seed_edition(
        &db,
        foundation_work,
        Some("Foundation (annotated)"),
        None,
        None,
        None,
    )
    .await;
    let dune_work = seed_work(&db, "Dune").await;
    let dune = seed_edition(&db, dune_work, Some("Dune"), None, None, None).await;

    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), Some(&db)).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let matches = matching_edition_ids(Some("Foundation".into()), None, app.state::<AppState>())
        .await
        .expect("matching editions");
    assert_eq!(sorted_ids(&matches), sorted_ids(&[first, second]));
    assert!(!matches.contains(&dune), "unrelated works are not returned");
}

#[tokio::test]
async fn matching_edition_ids_honours_the_has_file_filter() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let on_disk_work = seed_work(&db, "Foundation").await;
    let on_disk = seed_edition(&db, on_disk_work, Some("Foundation"), None, None, None).await;
    seed_inventory(&db, on_disk, Some("/books/foundation.epub".into())).await;
    let virtual_work = seed_work(&db, "Dune").await;
    // No inventory row at all: the index records this edition as `has_file = false`.
    let virtual_book = seed_edition(&db, virtual_work, Some("Dune"), None, None, None).await;

    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), Some(&db)).await;
    let app = tauri::test::mock_app();
    app.manage(state);

    let matches = matching_edition_ids(
        None,
        Some(EditionFilters {
            has_file: Some(true),
            ..EditionFilters::default()
        }),
        app.state::<AppState>(),
    )
    .await
    .expect("filtered matches");
    assert_eq!(matches, vec![on_disk], "only the edition that has a file");

    let everything = matching_edition_ids(None, None, app.state::<AppState>())
        .await
        .expect("unfiltered matches");
    assert_eq!(
        sorted_ids(&everything),
        sorted_ids(&[on_disk, virtual_book])
    );
}

#[tokio::test]
async fn matching_edition_ids_reports_a_missing_or_empty_index() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let work_id = seed_work(&db, "Foundation").await;
    seed_edition(&db, work_id, Some("Foundation"), None, None, None).await;

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);
    let error = matching_edition_ids(Some("Foundation".into()), None, app.state::<AppState>())
        .await
        .expect_err("no index must fail");
    assert_eq!(error.code, "index");
    assert_eq!(error.message, "search index unavailable");

    // An index that exists but was never filled has no work to match.
    let mut state = state_at(&test_db, &root).await;
    attach_index(&mut state, &root.join("index"), None).await;
    let app = tauri::test::mock_app();
    app.manage(state);
    let matches = matching_edition_ids(Some("Foundation".into()), None, app.state::<AppState>())
        .await
        .expect("empty index is not a failure");
    assert!(matches.is_empty());
}

// ---- export_editions_csv ---------------------------------------------------

#[tokio::test]
async fn export_editions_csv_renders_joined_metadata_and_rejects_missing_rows() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let language = languages::Entity::find()
        .one(&db)
        .await
        .expect("language query")
        .expect("seeded language");
    let format_id: DbId = KnownFormats::Epub.into();
    let format = formats::Entity::find_by_id(format_id)
        .one(&db)
        .await
        .expect("format query")
        .expect("seeded format");

    let work_id = seed_work(&db, "Dune").await;
    let date = time::Date::from_calendar_date(1965, time::Month::August, 1).expect("date");
    let edition_id = seed_edition(
        &db,
        work_id,
        Some("Dune, Part \"One\""),
        Some(format_id),
        Some(language.id),
        Some(date),
    )
    .await;

    let author_id = DbId::new();
    authors::ActiveModel {
        id: Set(author_id),
        name: Set("Frank Herbert".to_string()),
    }
    .insert(&db)
    .await
    .expect("author");
    edition_authors::ActiveModel {
        edition_id: Set(edition_id),
        author_id: Set(author_id),
        role: Set("aut".to_string()),
    }
    .insert(&db)
    .await
    .expect("edition author");

    for (value, kind) in [
        ("urn:isbn:9780441013593", "ISBN"),
        ("urn:wikidata:Q190192", "wikidata"),
    ] {
        let identifier_id = DbId::new();
        identifiers::ActiveModel {
            id: Set(identifier_id),
            value: Set(value.to_string()),
            kind: Set(kind.to_string()),
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

    let publisher_id = DbId::new();
    publishers::ActiveModel {
        id: Set(publisher_id),
        name: Set("Chilton".to_string()),
        website: Set(None),
        logo_url: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(&db)
    .await
    .expect("publisher");
    edition_publishers::ActiveModel {
        edition_id: Set(edition_id),
        publisher_id: Set(publisher_id),
    }
    .insert(&db)
    .await
    .expect("edition publisher");

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let out = root.join("export.csv");
    let outcome = export_editions_csv(vec![edition_id], out.to_string(), app.state::<AppState>())
        .await
        .expect("export");
    assert_eq!(outcome.rows, 1);
    assert_eq!(outcome.path, out.to_string());

    let body = fs_err::read_to_string(&out).expect("read export");
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "edition_id,work_id,title,authors,isbn,publisher,language,format,published_date"
    );
    assert_eq!(
        lines[1],
        format!(
            "{edition_id},{work_id},\"Dune, Part \"\"One\"\"\",Frank Herbert,urn:isbn:9780441013593,Chilton,{},{},1965-08-01",
            language.name, format.name
        )
    );
    assert!(
        !root.join("export.csv.tmp").exists(),
        "the temp file is renamed away"
    );

    let missing = root.join("missing.csv");
    let error = export_editions_csv(
        vec![DbId::new()],
        missing.to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("an unknown edition aborts the export");
    assert_eq!(error.code, "not_found");
    assert!(
        !missing.exists(),
        "nothing is written when a row cannot be rendered"
    );
}

// ---- create_virtual_book ---------------------------------------------------

#[tokio::test]
async fn create_virtual_book_writes_catalog_rows_and_no_file() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let format_id: DbId = KnownFormats::Epub.into();
    let format_name = formats::Entity::find_by_id(format_id)
        .one(&db)
        .await
        .expect("format query")
        .expect("seeded format")
        .name;
    let language_id: DbId = languages::Entity::find()
        .one(&db)
        .await
        .expect("language query")
        .expect("seeded language")
        .id;

    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let input = VirtualBookInput {
        title: "  Virtual Dune  ".to_string(),
        authors: vec![VirtualAuthor {
            name: " Frank Herbert ".to_string(),
            role: String::new(),
        }],
        publisher: Some(" Chilton ".to_string()),
        language_id: Some(language_id),
        format_id: Some(format_id),
        identifiers: vec![
            "978-1-78478-060-9".to_string(),
            "urn:oclc:12345".to_string(),
        ],
        description: Some("A catalog-only book".to_string()),
        notes: Some("no file on disk".to_string()),
        // Non-http covers are skipped without a network call.
        cover_image_url: Some("ftp://example.com/cover.jpg".to_string()),
    };

    let outcome = create_virtual_book(input, app.state::<AppState>())
        .await
        .expect("virtual book");
    assert_eq!(outcome.title, "Virtual Dune", "the title is trimmed");
    assert!(!outcome.duplicate);
    assert_eq!(outcome.isbns, vec!["9781784780609".to_string()]);

    let edition_id: DbId = outcome.edition_id.parse().expect("edition id");
    let edition = editions::Entity::find_by_id(edition_id)
        .one(&db)
        .await
        .expect("edition query")
        .expect("edition row");
    assert_eq!(edition.title.as_deref(), Some("Virtual Dune"));
    assert_eq!(edition.description.as_deref(), Some("A catalog-only book"));
    assert_eq!(edition.notes.as_deref(), Some("no file on disk"));
    assert_eq!(edition.format_id, Some(format_id));
    assert_eq!(edition.language_id, Some(language_id));

    let work = works::Entity::find_by_id(edition.work_id)
        .one(&db)
        .await
        .expect("work query")
        .expect("work row");
    assert_eq!(work.title, "Virtual Dune");
    assert_eq!(work.description.as_deref(), Some("A catalog-only book"));

    let author_links = edition_authors::Entity::find()
        .filter(edition_authors::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("author links");
    assert_eq!(author_links.len(), 1);
    assert_eq!(
        author_links[0].role, "aut",
        "an empty role defaults to `aut`"
    );
    let author = authors::Entity::find_by_id(author_links[0].author_id)
        .one(&db)
        .await
        .expect("author query")
        .expect("author row");
    assert_eq!(author.name, "Frank Herbert");

    let publisher_links = edition_publishers::Entity::find()
        .filter(edition_publishers::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("publisher links");
    assert_eq!(publisher_links.len(), 1);
    let publisher = publishers::Entity::find_by_id(publisher_links[0].publisher_id)
        .one(&db)
        .await
        .expect("publisher query")
        .expect("publisher row");
    assert_eq!(publisher.name, "Chilton");

    let links = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::EditionId.eq(edition_id))
        .all(&db)
        .await
        .expect("identifier links");
    let mut identifiers_seen: Vec<(String, String)> = Vec::new();
    for link in links {
        let identifier = identifiers::Entity::find_by_id(link.identifier_id)
            .one(&db)
            .await
            .expect("identifier query")
            .expect("identifier row");
        identifiers_seen.push((identifier.value, identifier.kind));
    }
    identifiers_seen.sort();
    assert_eq!(
        identifiers_seen,
        vec![
            ("urn:isbn:9781784780609".to_string(), "isbn".to_string()),
            ("urn:oclc:12345".to_string(), "other".to_string()),
        ],
        "the ISBN is canonicalized and stored as a `urn:isbn:` value"
    );

    let inventory = inventory_of(&db, edition_id).await;
    assert_eq!(inventory.file_path, None, "a virtual book has no file");
    assert_eq!(inventory.cover_path, None, "a non-http cover is skipped");
    assert_eq!(inventory.file_format.as_deref(), Some(format_name.as_str()));

    let error = create_virtual_book(virtual_input("   "), app.state::<AppState>())
        .await
        .expect_err("a title is required");
    assert_eq!(
        serde_json::to_value(&error).expect("serialize")["code"],
        "invalid"
    );
    assert_eq!(
        works::Entity::find().count(&db).await.expect("work count"),
        1,
        "a rejected input writes nothing"
    );
}

// ---- relink_edition_file ---------------------------------------------------

#[tokio::test]
async fn relink_edition_file_materializes_the_source_and_records_it() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());

    let work_id = seed_work(&db, "Relinkable").await;
    let edition_id = seed_edition(&db, work_id, Some("Relinkable"), None, None, None).await;
    seed_inventory(&db, edition_id, None).await;

    let source = root.join("source.epub");
    fs_err::write(&source, b"book bytes").expect("source file");

    let state = state_at(&test_db, &root).await;
    let books_dir = state.books_dir.clone();
    let app = tauri::test::mock_app();
    app.manage(state);

    let file = relink_edition_file(
        edition_id.to_string(),
        source.to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect("relink");

    let hash = hex::encode(sha2::Sha256::digest(b"book bytes"));
    let expected = books_dir.join(format!("{hash}-source.epub"));
    assert_eq!(file.file_path.as_deref(), Some(expected.as_str()));
    assert_eq!(file.file_format.as_deref(), Some("epub"));
    assert_eq!(file.file_size_bytes, Some(10.0));
    assert_eq!(file.cover_path, None);
    assert!(
        matches!(file.file_status, Some(FileStatus::Ok)),
        "a freshly materialized file is reachable"
    );
    assert!(
        expected.exists(),
        "the library entry is created at {expected}"
    );
    assert_eq!(
        fs_err::read(&expected).expect("read library file"),
        b"book bytes"
    );

    let inventory = inventory_of(&db, edition_id).await;
    assert_eq!(inventory.file_path.as_deref(), Some(expected.as_str()));
    assert_eq!(inventory.file_hash.as_deref(), Some(hash.as_str()));
    assert_eq!(inventory.file_size_bytes, Some(10));
    assert_eq!(inventory.file_format.as_deref(), Some("epub"));

    let error = relink_edition_file(
        "not-a-db-id".to_string(),
        source.to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("an unparseable id is rejected");
    assert_eq!(
        serde_json::to_value(&error).expect("serialize")["code"],
        "database"
    );

    let bare_edition = seed_edition(&db, work_id, Some("Bare"), None, None, None).await;
    let error = relink_edition_file(
        bare_edition.to_string(),
        source.to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("an edition without an inventory row cannot be relinked");
    assert!(
        format!("{error}").contains("no file recorded for this edition"),
        "unexpected error: {error}"
    );
}

// ---- sync_shelf_placement --------------------------------------------------

#[tokio::test]
async fn sync_shelf_placement_rejects_an_unknown_shelf_key() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let dir = tempfile::tempdir().expect("temp dir");
    let root = utf8(dir.path());
    let app = tauri::test::mock_app();
    app.manage(state_at(&test_db, &root).await);

    let error = sync_shelf_placement(
        DbId::new().to_string(),
        "nonsense".to_string(),
        "Dune".to_string(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("an unknown shelf key is rejected");

    assert_eq!(
        serde_json::to_value(&error).expect("serialize")["code"],
        "shelf"
    );
    assert!(
        format!("{error}").contains("unknown shelf `nonsense`"),
        "unexpected error: {error}"
    );
}
