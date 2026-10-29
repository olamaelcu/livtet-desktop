//! Book reviews: local persistence and publication to the account's PDS.
//!
//! Local rows live in the desktop's own `reviews` table, created on first use
//! (`CREATE TABLE IF NOT EXISTS`). The catalog's `reading_progress` table is
//! owned by the core migration set, which ships as a git dependency and cannot
//! grow a table here, so reviews get a table of their own rather than being
//! smuggled into a column that other writers own.
//!
//! Publishing is best effort: SQLite is written first and never rolled back
//! when the PDS write fails, so a review is never lost to a network or sign-in
//! problem. [`ReviewSync`] reports which of the two happened.

use crate::types::AppState;
use livtet_core::data::orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use livtet_types::now_primitive;
use serde::Serialize;
use specta::Type;
use tauri::State;

/// Review backend errors, shaped like the other command errors.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ReviewError {
    /// Stable machine code: `invalid` | `database` | `auth`.
    pub code: String,
    pub message: String,
}

impl ReviewError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl From<livtet_core::data::orm::DbErr> for ReviewError {
    fn from(error: livtet_core::data::orm::DbErr) -> Self {
        ReviewError::new("database", error.to_string())
    }
}

/// One stored review.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LocalReview {
    pub id: String,
    pub edition_id: String,
    pub text: String,
    pub rating: Option<f64>,
    pub created_at: String,
    /// `at://` URI of the published copy, once it exists. The handle a later
    /// `com.atproto.repo.deleteRecord` needs, kept even though nothing deletes
    /// remotely yet.
    pub record_uri: Option<String>,
}

/// Outcome of publishing a review to the PDS.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct ReviewSync {
    /// `at://` URI of the review record now in the account's repository.
    pub record_uri: Option<String>,
    /// The reviewed book's page in the Livtet web app, when the edition was
    /// resolved — so the reader can go look at what they just published.
    pub book_page_url: Option<String>,
    /// Why the record was not published, when it was not.
    pub error: Option<String>,
}

/// The Livtet web app's URL for one book edition.
///
/// The web app routes by the edition's repo and rkey
/// (`/book/did:web:biblio.livtet.olamaelcu.net/ol.OL26224505M`), both of which
/// the edition record's `at://` URI already carries.
pub(crate) fn book_page_url(book_uri: &str) -> Option<String> {
    let (repo, _collection, rkey) = crate::atproto::review_records::parse_at_uri(book_uri)?;
    Some(format!(
        "{}/book/{repo}/{rkey}",
        crate::atproto::CLIENT_ORIGIN
    ))
}

/// Create the `reviews` table. Cheap and idempotent, so every reader of the
/// table calls it instead of relying on a migration that cannot exist here.
pub(crate) async fn ensure_reviews_table(db: &DatabaseConnection) -> Result<(), ReviewError> {
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "CREATE TABLE IF NOT EXISTS reviews (
            id TEXT PRIMARY KEY NOT NULL,
            edition_id TEXT NOT NULL,
            text TEXT NOT NULL,
            rating REAL,
            created_at TEXT NOT NULL,
            record_uri TEXT
        )"
        .to_string(),
    ))
    .await?;
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "CREATE INDEX IF NOT EXISTS idx_reviews_edition_id ON reviews (edition_id)".to_string(),
    ))
    .await?;
    ensure_record_uri_column(db).await
}

/// Add `record_uri` to a `reviews` table created before it existed.
///
/// `CREATE TABLE IF NOT EXISTS` leaves an older table untouched, so the column
/// is added separately. `PRAGMA table_info` is the guard: SQLite has no
/// `ADD COLUMN IF NOT EXISTS`, and a blind `ALTER TABLE` would fail on every
/// start after the first.
pub(crate) async fn ensure_record_uri_column(db: &DatabaseConnection) -> Result<(), ReviewError> {
    let columns = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA table_info(reviews)".to_string(),
        ))
        .await?;
    let has_record_uri = columns.iter().any(|column| {
        column
            .try_get::<String>("", "name")
            .is_ok_and(|name| name == "record_uri")
    });
    if has_record_uri {
        return Ok(());
    }
    db.execute_raw(Statement::from_string(
        DbBackend::Sqlite,
        "ALTER TABLE reviews ADD COLUMN record_uri TEXT".to_string(),
    ))
    .await?;
    Ok(())
}

/// RFC 3339 text for a stored timestamp.
fn timestamp_text() -> String {
    use time::format_description::well_known::Rfc3339;
    let now = now_primitive();
    now.assume_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| now.to_string())
}

/// Store the edition's single review: insert the first time, update the
/// existing row afterwards. One row per book makes "write" and "edit" the
/// same operation and gives publishing one stable record to upsert.
///
/// `text` is trimmed and required; `rating` is clamped to the 0–10 half-step
/// scale and dropped when not finite. A row's `id`, `created_at` and published
/// `record_uri` survive an update. Leftover duplicates from older builds are
/// collapsed into the kept row (a published row first, then the newest).
pub(crate) async fn store_local_review(
    db: &DatabaseConnection,
    edition_id: &str,
    text: &str,
    rating: Option<f64>,
) -> Result<LocalReview, ReviewError> {
    ensure_reviews_table(db).await?;

    if edition_id.trim().is_empty() {
        return Err(ReviewError::new("invalid", "an edition id is required"));
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(ReviewError::new("invalid", "a review needs some text"));
    }
    let rating = rating.filter(|value| value.is_finite()).map(|value| {
        // The rating control offers halves in 0–10; anything else is clamped
        // so the record and the column always agree.
        (value.clamp(0.0, 10.0) * 2.0).round() / 2.0
    });

    let existing = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM reviews WHERE edition_id = ?
             ORDER BY (record_uri IS NULL) ASC, created_at DESC",
            [Value::from(edition_id.to_string())],
        ))
        .await?;

    let Some(kept) = existing.first() else {
        let review = LocalReview {
            id: livtet_types::DbId::new().to_hex(),
            edition_id: edition_id.to_string(),
            text: text.to_string(),
            rating,
            created_at: timestamp_text(),
            // Nothing is published yet; publishing fills this in.
            record_uri: None,
        };
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reviews (id, edition_id, text, rating, created_at, record_uri)
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                Value::from(review.id.clone()),
                Value::from(review.edition_id.clone()),
                Value::from(review.text.clone()),
                Value::from(review.rating),
                Value::from(review.created_at.clone()),
                Value::from(review.record_uri.clone()),
            ],
        ))
        .await?;
        return Ok(review);
    };

    let kept_id: String = kept.try_get("", "id")?;
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reviews SET text = ?, rating = ? WHERE id = ?",
        [
            Value::from(text.to_string()),
            Value::from(rating),
            Value::from(kept_id.clone()),
        ],
    ))
    .await?;
    // A review is 1:1 with its edition; drop rows left by older builds.
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM reviews WHERE edition_id = ? AND id <> ?",
        [
            Value::from(edition_id.to_string()),
            Value::from(kept_id.clone()),
        ],
    ))
    .await?;

    fetch_local_review(db, &kept_id).await
}

/// Every review stored for an edition, newest first.
pub(crate) async fn fetch_local_reviews(
    db: &DatabaseConnection,
    edition_id: &str,
) -> Result<Vec<LocalReview>, ReviewError> {
    ensure_reviews_table(db).await?;
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, edition_id, text, rating, created_at, record_uri FROM reviews
             WHERE edition_id = ? ORDER BY created_at DESC",
            [Value::from(edition_id.to_string())],
        ))
        .await?;

    let mut reviews = Vec::with_capacity(rows.len());
    for row in rows {
        reviews.push(LocalReview {
            id: row.try_get("", "id")?,
            edition_id: row.try_get("", "edition_id")?,
            text: row.try_get("", "text")?,
            rating: row.try_get("", "rating")?,
            created_at: row.try_get("", "created_at")?,
            record_uri: row.try_get("", "record_uri")?,
        });
    }
    Ok(reviews)
}

/// Read one stored review by id. Fails with `not-found` when the row is gone.
pub(crate) async fn fetch_local_review(
    db: &DatabaseConnection,
    review_id: &str,
) -> Result<LocalReview, ReviewError> {
    ensure_reviews_table(db).await?;
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, edition_id, text, rating, created_at, record_uri FROM reviews
             WHERE id = ?",
            [Value::from(review_id.to_string())],
        ))
        .await?;
    let Some(row) = rows.into_iter().next() else {
        return Err(ReviewError::new("not-found", "no review with that id"));
    };
    Ok(LocalReview {
        id: row.try_get("", "id")?,
        edition_id: row.try_get("", "edition_id")?,
        text: row.try_get("", "text")?,
        rating: row.try_get("", "rating")?,
        created_at: row.try_get("", "created_at")?,
        record_uri: row.try_get("", "record_uri")?,
    })
}

/// The record key a local review publishes to: the rkey of the record it was
/// already published as, else the local review id. Stable per row, so
/// re-publishing (including after an edit) upserts that one record.
pub(crate) fn review_rkey(review: &LocalReview) -> Result<String, ReviewError> {
    if let Some(uri) = review.record_uri.as_deref()
        && let Some((_, _, rkey)) = crate::atproto::review_records::parse_at_uri(uri)
    {
        return Ok(rkey.to_string());
    }
    Ok(review.id.clone())
}

/// Remember where a review was published, so the remote copy can be deleted
/// later without re-deriving it.
pub(crate) async fn attach_record_uri(
    db: &DatabaseConnection,
    review_id: &str,
    record_uri: &str,
) -> Result<(), ReviewError> {
    ensure_reviews_table(db).await?;
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE reviews SET record_uri = ? WHERE id = ?",
        [
            Value::from(record_uri.to_string()),
            Value::from(review_id.to_string()),
        ],
    ))
    .await?;
    Ok(())
}

/// Delete one review from this desktop's table.
///
/// `false` when the row was already gone, so a repeated click is not an error
/// — the same contract as `remove_reader_bookmark`. The published copy is left
/// alone: it belongs to the account, and nothing deletes it yet.
pub(crate) async fn delete_local_review(
    db: &DatabaseConnection,
    review_id: &str,
) -> Result<bool, ReviewError> {
    ensure_reviews_table(db).await?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "DELETE FROM reviews WHERE id = ?",
            [Value::from(review_id.to_string())],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Save a review locally. The review survives even when publishing fails.
#[tauri::command]
#[specta::specta]
pub async fn save_local_review(
    edition_id: String,
    text: String,
    rating: Option<f64>,
    state: State<'_, AppState>,
) -> Result<LocalReview, ReviewError> {
    store_local_review(&state.db.db_conn(), &edition_id, &text, rating).await
}

/// Stored reviews for an edition, newest first.
#[tauri::command]
#[specta::specta]
pub async fn load_local_reviews(
    edition_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LocalReview>, ReviewError> {
    fetch_local_reviews(&state.db.db_conn(), &edition_id).await
}

/// Delete one stored review.
///
/// Only the local row goes: a published copy lives in the account's repository
/// and is left in place, because removing it is a separate remote write the
/// user has not asked for. `false` when the row was already gone.
#[tauri::command]
#[specta::specta]
pub async fn remove_local_review(
    review_id: String,
    state: State<'_, AppState>,
) -> Result<bool, ReviewError> {
    delete_local_review(&state.db.db_conn(), &review_id).await
}

/// Resolve the book record's CID for the review's strong reference.
///
/// Anonymous: the book record lives on the platform's own repository and is
/// public, so reading it needs no scope. The AppView answers the standard
/// `com.atproto.repo.getRecord` route.
pub(crate) async fn resolve_book_cid(
    http: &reqwest::Client,
    book_uri: &str,
) -> Result<String, String> {
    let (repo, collection, rkey) = crate::atproto::review_records::parse_at_uri(book_uri)
        .ok_or_else(|| format!("book reference is not an at:// record URI: {book_uri}"))?;
    let response = http
        .get(format!(
            "{}/xrpc/com.atproto.repo.getRecord",
            crate::atproto::APPVIEW_URL
        ))
        .query(&[("repo", repo), ("collection", collection), ("rkey", rkey)])
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("could not reach the book index: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "the book index answered {} for {book_uri}",
            response.status()
        ));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("the book index returned an unreadable record: {e}"))?;
    body.get("cid")
        .and_then(|cid| cid.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("the book record for {book_uri} carries no cid"))
}

/// Publish a review to the account's PDS as a `net.olamaelcu.livtet.bookReview`
/// record.
///
/// The caller saves locally first; this command only reports what publishing
/// did, so a failure never costs the reader their words.
pub(crate) async fn publish_review(
    http: &reqwest::Client,
    manager: &crate::atproto::AtprotoManager,
    rkey: &str,
    title: &str,
    text: &str,
    rating: Option<f64>,
) -> ReviewSync {
    let title = title.trim();
    if title.is_empty() {
        return ReviewSync {
            record_uri: None,
            book_page_url: None,
            error: Some("This edition has no title to look up in the book index".to_string()),
        };
    }
    let (book_uri, _index_title) = match super::import::resolve_edition_record(http, title).await {
        Ok(found) => found,
        Err(error) => {
            return ReviewSync {
                record_uri: None,
                book_page_url: None,
                error: Some(error),
            };
        }
    };
    // Resolved once and reused: the workspace row shows the same link whether
    // or not publishing succeeds.
    let page_url = book_page_url(&book_uri);
    let book_cid = match resolve_book_cid(http, &book_uri).await {
        Ok(cid) => cid,
        Err(error) => {
            return ReviewSync {
                record_uri: None,
                book_page_url: page_url,
                error: Some(error),
            };
        }
    };
    // The published rating is the platform's 1-10 half-step scale; an unrated
    // review omits the field entirely.
    let published_rating = rating
        .filter(|value| value.is_finite())
        .map(|value| (value.clamp(1.0, 10.0) * 2.0).round() / 2.0);
    match manager
        .write_review(&book_uri, &book_cid, rkey, text, published_rating)
        .await
    {
        Ok(uri) => ReviewSync {
            record_uri: Some(uri),
            book_page_url: page_url,
            error: None,
        },
        Err(error) => {
            tracing::warn!(%error, "publishing the review failed");
            ReviewSync {
                record_uri: None,
                book_page_url: page_url,
                error: Some(error.to_string()),
            }
        }
    }
}

/// Publish a review to the user's PDS. Reports rather than fails when the
/// account is signed out or the index cannot resolve the edition.
///
/// On success the record's `at://` URI is written back onto the local row
/// (`reviews.record_uri`), which is the handle a later remote delete needs.
#[tauri::command]
#[specta::specta]
pub async fn put_local_review_to_pds(
    review_id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<ReviewSync, ReviewError> {
    let db = state.db.db_conn();
    // The stored row is the source of truth: an edit followed by a publish must
    // send the words and rating the reader just saved, not stale arguments.
    let review = fetch_local_review(&db, &review_id).await?;
    let rkey = review_rkey(&review)?;
    let sync = publish_review(
        &state.opds_http,
        &state.atproto,
        &rkey,
        &title,
        &review.text,
        review.rating,
    )
    .await;
    if let Some(uri) = sync.record_uri.as_deref() {
        // A failure to remember the URI must not turn a published review into
        // an error the reader sees; the review itself is already on the PDS.
        if let Err(error) = attach_record_uri(&db, &review_id, uri).await {
            tracing::warn!(
                ?error,
                review_id,
                "could not store the published review's URI"
            );
        }
    }
    Ok(sync)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use livtet_core::data::{Kind, TestDb};

    async fn db() -> (TestDb, DatabaseConnection) {
        let test = TestDb::new(&[Kind::Business]).await.unwrap();
        let conn = test.state().db_conn();
        (test, conn)
    }

    #[tokio::test]
    async fn a_review_round_trips_through_sqlite() {
        let (_guard, db) = db().await;
        let saved = store_local_review(&db, "edition-1", "  Slow start.  ", Some(7.5))
            .await
            .unwrap();
        assert_eq!(saved.text, "Slow start.", "text is trimmed");
        assert_eq!(saved.rating, Some(7.5));

        let found = fetch_local_reviews(&db, "edition-1").await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], saved);
        assert!(
            fetch_local_reviews(&db, "edition-2")
                .await
                .unwrap()
                .is_empty(),
            "reviews are scoped to their edition"
        );
    }

    #[tokio::test]
    async fn an_unrated_review_stores_a_null_rating() {
        let (_guard, db) = db().await;
        let saved = store_local_review(&db, "edition-1", "No stars.", None)
            .await
            .unwrap();
        assert_eq!(saved.rating, None);
        assert_eq!(
            fetch_local_reviews(&db, "edition-1").await.unwrap()[0].rating,
            None
        );
    }

    #[tokio::test]
    async fn ratings_are_clamped_to_the_ten_point_scale() {
        let (_guard, db) = db().await;
        let over = store_local_review(&db, "e", "Too many stars.", Some(42.0))
            .await
            .unwrap();
        assert_eq!(over.rating, Some(10.0));
        let under = store_local_review(&db, "e", "Under.", Some(-3.0))
            .await
            .unwrap();
        assert_eq!(under.rating, Some(0.0));
    }

    #[tokio::test]
    async fn a_table_created_before_the_uri_column_gains_it() {
        let (_guard, db) = db().await;
        // The shape this code shipped with before `record_uri` existed.
        db.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            "CREATE TABLE reviews (
                id TEXT PRIMARY KEY NOT NULL,
                edition_id TEXT NOT NULL,
                text TEXT NOT NULL,
                rating REAL,
                created_at TEXT NOT NULL
            )"
            .to_string(),
        ))
        .await
        .unwrap();

        ensure_reviews_table(&db).await.unwrap();
        let saved = store_local_review(&db, "edition-1", "Still readable.", Some(6.0))
            .await
            .unwrap();
        assert_eq!(saved.record_uri, None);
        assert_eq!(
            fetch_local_reviews(&db, "edition-1").await.unwrap().len(),
            1
        );

        // Idempotent: a second call on the migrated table must not fail.
        ensure_reviews_table(&db).await.unwrap();
    }

    #[tokio::test]
    async fn publishing_records_the_uri_on_the_row() {
        let (_guard, db) = db().await;
        let saved = store_local_review(&db, "edition-1", "Published.", Some(7.0))
            .await
            .unwrap();

        attach_record_uri(
            &db,
            &saved.id,
            "at://did:plc:x/net.olamaelcu.livtet.bookReview/3l3",
        )
        .await
        .unwrap();

        let found = fetch_local_reviews(&db, "edition-1").await.unwrap();
        assert_eq!(
            found[0].record_uri.as_deref(),
            Some("at://did:plc:x/net.olamaelcu.livtet.bookReview/3l3")
        );
        assert_eq!(found[0].text, "Published.");
    }

    #[tokio::test]
    async fn deleting_removes_only_that_edition_s_review() {
        let (_guard, db) = db().await;
        let review = store_local_review(&db, "edition-1", "Only one.", None)
            .await
            .unwrap();
        let other = store_local_review(&db, "edition-2", "Other book.", None)
            .await
            .unwrap();

        assert!(delete_local_review(&db, &review.id).await.unwrap());

        assert!(
            fetch_local_reviews(&db, "edition-1")
                .await
                .unwrap()
                .is_empty(),
            "the edition's only review is gone"
        );
        assert_eq!(
            fetch_local_reviews(&db, "edition-2").await.unwrap()[0].id,
            other.id,
            "another edition keeps its review"
        );

        assert!(
            !delete_local_review(&db, &review.id).await.unwrap(),
            "a repeated delete reports that the row was already gone"
        );
    }

    #[tokio::test]
    async fn writing_twice_updates_the_one_review() {
        let (_guard, db) = db().await;
        let first = store_local_review(&db, "edition-1", "First.", Some(4.0))
            .await
            .unwrap();
        let second = store_local_review(&db, "edition-1", "  Second.  ", Some(9.5))
            .await
            .unwrap();

        assert_eq!(second.id, first.id, "one review per book");
        assert_eq!(second.text, "Second.");
        assert_eq!(second.rating, Some(9.5));
        assert_eq!(
            second.created_at, first.created_at,
            "created_at survives an edit"
        );
        let found = fetch_local_reviews(&db, "edition-1").await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], second);
    }

    #[tokio::test]
    async fn leftover_duplicates_collapse_into_one_row() {
        let (_guard, db) = db().await;
        let kept = store_local_review(&db, "edition-1", "Kept.", None)
            .await
            .unwrap();
        // A row an older build could have left behind, with an older timestamp.
        db.execute_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "INSERT INTO reviews (id, edition_id, text, rating, created_at, record_uri)
             VALUES (?, ?, ?, ?, ?, ?)",
            [
                Value::from("01dup".to_string()),
                Value::from("edition-1".to_string()),
                Value::from("Duplicate.".to_string()),
                Value::from(None::<f64>),
                Value::from("2026-01-01T00:00:00Z".to_string()),
                Value::from(None::<String>),
            ],
        ))
        .await
        .unwrap();

        let saved = store_local_review(&db, "edition-1", "Final.", Some(3.0))
            .await
            .unwrap();
        assert_eq!(saved.id, kept.id);
        assert_eq!(
            fetch_local_reviews(&db, "edition-1").await.unwrap().len(),
            1
        );
    }

    #[test]
    fn review_rkey_prefers_the_published_record() {
        let mut review = LocalReview {
            id: "01abc".to_string(),
            edition_id: "e".to_string(),
            text: "t".to_string(),
            rating: None,
            created_at: "c".to_string(),
            record_uri: None,
        };
        assert_eq!(review_rkey(&review).unwrap(), "01abc");
        review.record_uri = Some("at://did:plc:x/net.olamaelcu.livtet.bookReview/3l3".to_string());
        assert_eq!(review_rkey(&review).unwrap(), "3l3");
    }

    #[test]
    fn the_book_page_url_mirrors_the_web_app_route() {
        assert_eq!(
            book_page_url(
                "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL26224505M"
            )
            .as_deref(),
            Some("https://livtet.olamaelcu.net/book/did:web:biblio.livtet.olamaelcu.net/ol.OL26224505M")
        );
        assert_eq!(book_page_url("not-a-record-uri"), None);
    }

    #[tokio::test]
    async fn an_empty_review_is_rejected() {
        let (_guard, db) = db().await;
        assert!(store_local_review(&db, "e", "   ", None).await.is_err());
        assert!(store_local_review(&db, "", "text", None).await.is_err());
    }
}
