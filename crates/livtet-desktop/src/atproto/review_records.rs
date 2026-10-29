//! Record bodies for reviews, matching the records the platform writes.
//!
//! The shape is the platform's `BookReviewRecordSchema`
//! (`platform/packages/livtet-service/src/lib/bibliograph/schemas.ts`) and the
//! collection is one the account's OAuth grant already covers — writing into a
//! different collection fails with `InsufficientScope` no matter how correct
//! the body is.
//!
//! ```json
//! // net.olamaelcu.livtet.bookReview/3l3qo2vqk2s2a
//! { "$type": "net.olamaelcu.livtet.bookReview",
//!   "text": "…",
//!   "book": { "uri": "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M",
//!             "cid": "bafyrei…" },
//!   "rating": 8.5,
//!   "createdAt": "2026-10-28T09:12:00.000Z" }
//! ```

use serde_json::{Value, json};

/// The collection the platform index reads, and the one the OAuth grant allows.
pub(crate) const REVIEW_COLLECTION: &str = "net.olamaelcu.livtet.bookReview";

/// Split an `at://` record URI into `(repo, collection, rkey)`.
pub(crate) fn parse_at_uri(uri: &str) -> Option<(&str, &str, &str)> {
    let rest = uri.strip_prefix("at://")?;
    let mut parts = rest.splitn(3, '/');
    let repo = parts.next()?;
    let collection = parts.next()?;
    let rkey = parts.next()?;
    if repo.is_empty() || collection.is_empty() || rkey.is_empty() || rkey.contains('/') {
        return None;
    }
    Some((repo, collection, rkey))
}

/// The body of a review record.
///
/// `rating` is `None` when the reader left the stars alone; the field is then
/// omitted rather than sent as `null`, because the platform schema types it as
/// an optional number and the index rejects a null.
pub(crate) fn review_record(
    book_uri: &str,
    book_cid: &str,
    text: &str,
    rating: Option<f64>,
    now: &str,
) -> Value {
    let mut record = json!({
        "$type": REVIEW_COLLECTION,
        "text": text,
        "book": { "uri": book_uri, "cid": book_cid },
        "createdAt": now,
    });
    if let Some(rating) = rating {
        record["rating"] = json!(rating);
    }
    record
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const BOOK: &str =
        "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M";
    const CID: &str = "bafyreicid";
    const NOW: &str = "2026-10-28T09:12:00.000Z";

    #[test]
    fn review_record_matches_the_platform_schema() {
        let record = review_record(BOOK, CID, "Slow start, great ending.", Some(8.5), NOW);
        assert_eq!(record["$type"], "net.olamaelcu.livtet.bookReview");
        assert_eq!(record["text"], "Slow start, great ending.");
        assert_eq!(record["book"]["uri"], BOOK);
        assert_eq!(record["book"]["cid"], CID);
        assert_eq!(record["rating"], 8.5);
        assert_eq!(record["createdAt"], NOW);
        // The platform schema has no stronger fields: an extra key would be a
        // different record than the index knows how to read.
        let keys: Vec<&String> = record.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 5, "unexpected fields: {keys:?}");
    }

    #[test]
    fn an_unrated_review_omits_the_rating_field() {
        let record = review_record(BOOK, CID, "No stars from me.", None, NOW);
        assert!(
            record.get("rating").is_none(),
            "rating must be omitted, not null"
        );
    }

    #[test]
    fn parse_at_uri_splits_repo_collection_and_rkey() {
        assert_eq!(
            parse_at_uri(BOOK),
            Some((
                "did:web:biblio.livtet.olamaelcu.net",
                "community.lexicon.book.edition",
                "ol.OL997340M"
            ))
        );
        assert_eq!(
            parse_at_uri("at://did:plc:x/coll/rkey"),
            Some(("did:plc:x", "coll", "rkey"))
        );
        assert_eq!(parse_at_uri("at://did:plc:x/coll"), None);
        assert_eq!(parse_at_uri("https://example.com/x"), None);
        assert_eq!(parse_at_uri("at://did:plc:x/coll/a/b"), None);
    }
}
