//! `list_shelved_books` / `import_shelved_book` — offer the books the signed-in
//! account shelves in its AT Protocol repository as local virtual books.
//!
//! The records come from the account's own PDS (one `bookShelving` record per
//! shelved edition, across every shelf), which is the source of truth; the
//! appview's shelf endpoints index a lagging copy. Importing copies catalog
//! metadata from the book index, mints a local virtual edition, and records the
//! shelf the book was on. Nothing is written back to the PDS: the shelving
//! record already exists there.

use livtet_core::data::{
    entities::{edition_identifiers, editions, identifiers},
    orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter},
};
use livtet_core::shelf::ReadingStatus;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::import::{
    AuthorRequirement, ImportError, ImportOutcome, VirtualAuthor, VirtualBookInput,
    create_virtual_book_and_index, search_biblio_editions_inner,
};
use crate::atproto::shelf_records::ShelvingRecord;
use crate::error::AtprotoError;
use crate::types::AppState;

/// Drawer order of the four shelves; an unknown shelf sorts last.
const SHELF_ORDER: [&str; 4] = ["reading", "read", "to-read", "dnf"];

/// One book the account has shelved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct ShelvedBook {
    /// `at://` URI of the edition record the shelving record points at.
    pub book_uri: String,
    pub title: String,
    /// Reading status key (`reading`, `read`, `to-read`, `dnf`) when the record
    /// names one this app has.
    pub shelf_key: Option<String>,
    /// Human label for that shelf (`Finished`, `Want to Read`, …).
    pub shelf_name: Option<String>,
    /// When the account shelved the book (`createdAt`), verbatim.
    pub shelved_at: Option<String>,
}

/// Every shelved book in the signed-in account's repository.
#[tauri::command]
#[specta::specta]
pub async fn list_shelved_books(
    state: State<'_, AppState>,
) -> Result<Vec<ShelvedBook>, ImportError> {
    let records = state
        .inner()
        .atproto
        .list_shelving_records()
        .await
        .map_err(atproto_error)?;
    Ok(shelved_books(records))
}

/// Map stored shelving records to drawer rows: shelf order, then title.
pub(crate) fn shelved_books(records: Vec<ShelvingRecord>) -> Vec<ShelvedBook> {
    let mut rows: Vec<ShelvedBook> = records
        .into_iter()
        .map(|record| ShelvedBook {
            book_uri: record.book_uri,
            title: record.title,
            shelf_name: record
                .shelf_key
                .and_then(ReadingStatus::from_key)
                .map(|status| status.shelf_name())
                .map(str::to_string),
            shelf_key: record.shelf_key.map(str::to_string),
            shelved_at: record.added_at,
        })
        .collect();
    rows.sort_by(|left, right| {
        let rank = |row: &ShelvedBook| {
            row.shelf_key
                .as_deref()
                .and_then(|key| SHELF_ORDER.iter().position(|order| *order == key))
                .unwrap_or(SHELF_ORDER.len())
        };
        rank(left)
            .cmp(&rank(right))
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
    });
    rows
}

/// Import one shelved book as a local virtual book, recording its shelf.
#[tauri::command]
#[specta::specta]
pub async fn import_shelved_book(
    book: ShelvedBook,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, ImportError> {
    let did = state
        .inner()
        .atproto
        .get_current_did()
        .await
        .map_err(atproto_error)?;
    let title = book.title.trim().to_string();
    if title.is_empty() {
        return Err(ImportError::new("invalid", "a shelved book needs a title"));
    }
    let input = shelf_book_input(&state.opds_http, &book, title).await;
    import_shelved_book_inner(
        &state.db,
        &state.covers_dir,
        &state.opds_http,
        &state.search_index,
        &did,
        book,
        input,
    )
    .await
}

/// Import a shelved book: catalog rows, search index, then the shelf it was on.
///
/// `input` is the catalog metadata the caller already resolved (the enrichment
/// lookup is network work, so it stays outside this transaction-bounded path).
/// Kept free of Tauri state so it can be unit-tested against a `TestDb`.
#[tracing::instrument(skip_all, fields(book_uri = %book.book_uri), ret, err)]
pub(crate) async fn import_shelved_book_inner(
    db: &livtet_core::data::SharedState,
    covers_dir: &camino::Utf8Path,
    http: &reqwest::Client,
    search_index: &tokio::sync::RwLock<Option<livtet_core::search::SearchIndex>>,
    did: &str,
    book: ShelvedBook,
    input: VirtualBookInput,
) -> Result<ImportOutcome, ImportError> {
    if input.title.trim().is_empty() {
        return Err(ImportError::new("invalid", "a shelved book needs a title"));
    }
    let conn = db.db_conn();

    // The edition URI is stored as an identifier, so importing the same shelved
    // book twice returns the edition the first import made.
    if let Some(existing) = edition_for_book_uri(&conn, &book.book_uri).await? {
        record_shelf(&conn, did, &book, &existing.edition_id).await?;
        return Ok(existing);
    }

    let outcome = create_virtual_book_and_index(
        db,
        covers_dir,
        http,
        search_index,
        input,
        AuthorRequirement::Optional,
    )
    .await?;
    record_shelf(&conn, did, &book, &outcome.edition_id).await?;
    Ok(outcome)
}

/// The virtual edition a shelved book URI was already imported as, if any.
async fn edition_for_book_uri(
    conn: &DatabaseConnection,
    book_uri: &str,
) -> Result<Option<ImportOutcome>, ImportError> {
    let Some(identifier) = identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(book_uri))
        .one(conn)
        .await?
    else {
        return Ok(None);
    };
    let Some(link) = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::IdentifierId.eq(identifier.id))
        .one(conn)
        .await?
    else {
        return Ok(None);
    };
    let Some(edition) = editions::Entity::find_by_id(link.edition_id)
        .one(conn)
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(ImportOutcome {
        work_id: edition.work_id.to_string(),
        edition_id: edition.id.to_string(),
        title: edition.title.unwrap_or_default(),
        isbns: Vec::new(),
        duplicate: true,
    }))
}

/// Copy the shelf the account had the book on into the local shelf tables.
///
/// An unknown shelf key means the record names a status this app does not
/// model, so the book is imported without a shelf rather than guessed at.
async fn record_shelf(
    conn: &DatabaseConnection,
    did: &str,
    book: &ShelvedBook,
    edition_id: &str,
) -> Result<(), ImportError> {
    let Some(status) = book.shelf_key.as_deref().and_then(ReadingStatus::from_key) else {
        tracing::debug!(
            shelf = book.shelf_key.as_deref().unwrap_or("none"),
            "shelved book has no modeled reading status; importing without a shelf"
        );
        return Ok(());
    };
    let shelf_uri = format!("shelf:{}", status.as_str());
    livtet_core::shelf::ensure_shelf(conn, &shelf_uri, status.shelf_name())
        .await
        .map_err(|e| ImportError::new("db", format!("creating shelf: {e}")))?;
    livtet_core::shelf::upsert_book_shelf(conn, did, &shelf_uri, edition_id, status)
        .await
        .map_err(|e| ImportError::new("db", format!("writing shelf row: {e}")))?;
    Ok(())
}

/// Build the virtual-book input for a shelving record.
///
/// The record carries only a title and the edition URI, so catalog metadata
/// comes from the book index. The exact edition record wins; a hit is accepted
/// on title only when no exact record matched, so a different book with the same
/// title cannot contribute its author or cover. A failed lookup still imports the
/// book: the shelf entry is the thing the user asked to keep.
async fn shelf_book_input(
    http: &reqwest::Client,
    book: &ShelvedBook,
    title: String,
) -> VirtualBookInput {
    let hits = search_biblio_editions_inner(http, &title, 5)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(
                %error,
                %title,
                "book index lookup failed; importing the shelved book without metadata"
            );
            Vec::new()
        });
    shelved_book_input(&hits, book, title)
}

/// Pure half of [`shelf_book_input`]: pick the metadata hit and build the input.
fn shelved_book_input(
    hits: &[super::import::BiblioEdition],
    book: &ShelvedBook,
    title: String,
) -> VirtualBookInput {
    let matched = hits
        .iter()
        .find(|hit| hit.uri.as_deref() == Some(book.book_uri.as_str()))
        .or_else(|| {
            hits.iter()
                .find(|hit| hit.title.trim().eq_ignore_ascii_case(title.trim()))
        });

    VirtualBookInput {
        title,
        authors: matched
            .map(|hit| {
                hit.authors
                    .iter()
                    .map(|author| VirtualAuthor {
                        name: author.name.clone(),
                        role: author.role.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        publisher: None,
        language_id: None,
        format_id: None,
        // The edition record URI is what the shelving record points at, and the
        // key that makes a second import of the same book a duplicate.
        identifiers: vec![book.book_uri.clone()],
        description: None,
        notes: None,
        cover_image_url: matched.and_then(|hit| hit.cover_image_url.clone()),
    }
}

/// Translate an ATProto failure into the import error surface the drawer reads.
fn atproto_error(error: AtprotoError) -> ImportError {
    match error {
        AtprotoError::NotSignedIn => ImportError::new(
            "atproto",
            "sign in to your ATProto account to import your shelves",
        ),
        other => ImportError::new("atproto", other),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use livtet_core::data::entities::digital_inventory;
    use livtet_core::data::{Kind, TestDb};

    use super::*;
    use crate::commands::import::{BiblioContributorSummary, BiblioEdition};
    use livtet_types::DbId;

    const BOOK_URI: &str =
        "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M";
    const DID: &str = "did:plc:e2ctbutx6kya6si4if5ngjmm";
    const TITLE: &str = "Whores and other feminists";

    fn shelved(title: &str, shelf_key: Option<&str>) -> ShelvedBook {
        ShelvedBook {
            book_uri: BOOK_URI.to_string(),
            title: title.to_string(),
            shelf_key: shelf_key.map(str::to_string),
            shelf_name: shelf_key
                .and_then(ReadingStatus::from_key)
                .map(|status| status.shelf_name().to_string()),
            shelved_at: Some("2026-10-24T23:54:31.707Z".to_string()),
        }
    }

    fn hit(uri: &str, title: &str, author: &str) -> BiblioEdition {
        BiblioEdition {
            uri: Some(uri.to_string()),
            title: title.to_string(),
            cover_image_url: Some("https://covers.example/1.jpg".to_string()),
            published_year: Some(1997),
            language: Some("eng".to_string()),
            authors: vec![BiblioContributorSummary {
                name: author.to_string(),
                role: "aut".to_string(),
            }],
            identifiers: Vec::new(),
        }
    }

    fn record(book_uri: &str, title: &str, shelf_key: Option<&'static str>) -> ShelvingRecord {
        ShelvingRecord {
            book_uri: book_uri.to_string(),
            title: title.to_string(),
            shelf_key,
            added_at: None,
        }
    }

    #[test]
    fn rows_sort_by_shelf_order_then_title() {
        let rows = shelved_books(vec![
            record("at://a/1", "Zebra", Some("dnf")),
            record("at://a/2", "Ant", Some("read")),
            record("at://a/3", "Bee", Some("reading")),
            record("at://a/4", "Unknown shelf", None),
            record("at://a/5", "Annotated", Some("read")),
        ]);
        let titles: Vec<&str> = rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Bee", "Annotated", "Ant", "Zebra", "Unknown shelf"]
        );
        assert_eq!(rows[0].shelf_name.as_deref(), Some("Currently Reading"));
        assert_eq!(rows[1].shelf_name.as_deref(), Some("Finished"));
        assert_eq!(rows[4].shelf_name, None);
        assert_eq!(rows[4].shelf_key, None);
    }

    #[test]
    fn the_exact_edition_record_wins_over_a_same_titled_hit() {
        let hits = vec![
            hit("at://other/edition/1", TITLE, "Wrong Author"),
            hit(BOOK_URI, TITLE, "Right Author"),
        ];
        let input = shelved_book_input(&hits, &shelved(TITLE, Some("read")), TITLE.to_string());
        assert_eq!(input.authors.len(), 1);
        assert_eq!(input.authors[0].name, "Right Author");
        assert_eq!(input.identifiers, vec![BOOK_URI.to_string()]);
        assert_eq!(
            input.cover_image_url.as_deref(),
            Some("https://covers.example/1.jpg")
        );
    }

    #[test]
    fn a_same_titled_hit_is_accepted_when_no_exact_record_matched() {
        let hits = vec![hit("at://other/edition/1", "  dune ", "Frank Herbert")];
        let input = shelved_book_input(&hits, &shelved("Dune", Some("to-read")), "Dune".into());
        assert_eq!(input.authors.len(), 1);
        assert_eq!(input.authors[0].name, "Frank Herbert");
    }

    #[test]
    fn a_hit_with_a_different_title_contributes_nothing() {
        let hits = vec![hit("at://other/edition/1", "Some Other Book", "Someone")];
        let input = shelved_book_input(&hits, &shelved("Spy", Some("read")), "Spy".into());
        assert!(input.authors.is_empty());
        assert!(input.cover_image_url.is_none());
        assert_eq!(input.identifiers, vec![BOOK_URI.to_string()]);
    }

    #[test]
    fn no_index_hit_still_yields_an_importable_input() {
        let book = shelved("Was Hinduism Invented?", Some("read"));
        let input = shelved_book_input(&[], &book, book.title.clone());
        assert!(input.authors.is_empty());
        assert!(input.cover_image_url.is_none());
        assert_eq!(input.title, "Was Hinduism Invented?");
        assert_eq!(input.identifiers, vec![BOOK_URI.to_string()]);
    }

    struct Fixture {
        _db: TestDb,
        state: livtet_core::data::SharedState,
        _covers: tempfile::TempDir,
        covers_dir: camino::Utf8PathBuf,
        index: tokio::sync::RwLock<Option<livtet_core::search::SearchIndex>>,
    }

    async fn fixture() -> Fixture {
        let db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = db.state();
        let covers = tempfile::tempdir().expect("temp dir");
        let covers_dir = camino::Utf8Path::from_path(covers.path())
            .expect("utf8 path")
            .to_path_buf();
        Fixture {
            _db: db,
            state,
            _covers: covers,
            covers_dir,
            index: tokio::sync::RwLock::new(None),
        }
    }

    /// The metadata the command resolves over the network, without the network.
    fn input(book: &ShelvedBook, authors: Vec<VirtualAuthor>) -> VirtualBookInput {
        VirtualBookInput {
            title: book.title.clone(),
            authors,
            publisher: None,
            language_id: None,
            format_id: None,
            identifiers: vec![book.book_uri.clone()],
            description: None,
            notes: None,
            cover_image_url: None,
        }
    }

    #[tokio::test]
    async fn importing_a_shelved_book_records_its_reading_status() {
        let fixture = fixture().await;
        let book = shelved(TITLE, Some("read"));
        let authors = vec![VirtualAuthor {
            name: "Susana M. Morris".to_string(),
            role: "aut".to_string(),
        }];

        let outcome = import_shelved_book_inner(
            &fixture.state,
            &fixture.covers_dir,
            &reqwest::Client::new(),
            &fixture.index,
            DID,
            book.clone(),
            input(&book, authors),
        )
        .await
        .expect("import");
        assert!(!outcome.duplicate);
        assert_eq!(outcome.title, TITLE);

        let conn = fixture.state.db_conn();
        let edition_id = outcome.edition_id.parse::<DbId>().expect("edition id");
        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(edition_id))
            .one(&conn)
            .await
            .expect("query inventory")
            .expect("inventory row");
        assert!(inventory.file_path.is_none(), "the book stays virtual");

        let placement = livtet_core::shelf::shelf_of_book(&conn, &outcome.edition_id)
            .await
            .expect("query shelf")
            .expect("shelf row");
        assert_eq!(placement.status, "read");
        assert_eq!(placement.did, DID);
    }

    #[tokio::test]
    async fn importing_the_same_book_twice_returns_the_first_edition() {
        let fixture = fixture().await;
        let book = shelved("Dune", Some("to-read"));
        let authors = vec![VirtualAuthor {
            name: "Frank Herbert".to_string(),
            role: "aut".to_string(),
        }];

        let first = import_shelved_book_inner(
            &fixture.state,
            &fixture.covers_dir,
            &reqwest::Client::new(),
            &fixture.index,
            DID,
            book.clone(),
            input(&book, authors.clone()),
        )
        .await
        .expect("first import");
        let second = import_shelved_book_inner(
            &fixture.state,
            &fixture.covers_dir,
            &reqwest::Client::new(),
            &fixture.index,
            DID,
            book.clone(),
            input(&book, authors),
        )
        .await
        .expect("second import");

        assert!(!first.duplicate);
        assert!(second.duplicate, "the edition URI is the dedup key");
        assert_eq!(second.edition_id, first.edition_id);

        let conn = fixture.state.db_conn();
        let rows = digital_inventory::Entity::find()
            .all(&conn)
            .await
            .expect("query inventory");
        assert_eq!(rows.len(), 1, "no second inventory row");
    }

    #[tokio::test]
    async fn a_book_on_an_unknown_shelf_is_imported_without_a_shelf_row() {
        let fixture = fixture().await;
        let book = shelved("Was Hinduism Invented?", None);

        let outcome = import_shelved_book_inner(
            &fixture.state,
            &fixture.covers_dir,
            &reqwest::Client::new(),
            &fixture.index,
            DID,
            book.clone(),
            input(&book, Vec::new()),
        )
        .await
        .expect("authorless import");

        let conn = fixture.state.db_conn();
        assert!(
            livtet_core::shelf::shelf_of_book(&conn, &outcome.edition_id)
                .await
                .expect("query shelf")
                .is_none()
        );
    }
}
