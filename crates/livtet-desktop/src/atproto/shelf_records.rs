//! Record bodies for shelving, written by the account itself.
//!
//! The shapes below are what the Bibliograph appview already writes
//! (`shelf-writer.ts`) and what the account's repository holds: a shelf record
//! keyed by reading status, and a shelving record keyed by the edition's rkey
//! that points at one shelf and carries the edition reference.
//!
//! ```json
//! // net.olamaelcu.livtet.biblio.shelf/to-read
//! { "$type": "net.olamaelcu.livtet.biblio.shelf",
//!   "name": "Want to Read", "createdAt": "…", "updatedAt": "…" }
//!
//! // net.olamaelcu.livtet.biblio.bookShelving/ol.OL997340M
//! { "$type": "net.olamaelcu.livtet.biblio.bookShelving",
//!   "shelf": "at://did:plc:…/net.olamaelcu.livtet.biblio.shelf/to-read",
//!   "book": { "ref": "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M",
//!             "title": "Wh…" },
//!   "metadata": { "status": "to-read" },
//!   "createdAt": "…", "updatedAt": "…" }
//! ```

use livtet_core::shelf::ReadingStatus;
use serde_json::{Value, json};

/// Shelf records (one per reading status) live in this collection.
pub(crate) const SHELF_COLLECTION: &str = "net.olamaelcu.livtet.biblio.shelf";
/// One record per shelved edition.
pub(crate) const BOOK_SHELVING_COLLECTION: &str = "net.olamaelcu.livtet.biblio.bookShelving";

/// `at://<did>/net.olamaelcu.livtet.biblio.shelf/<status>` — the shelf a
/// shelving record points at.
pub(crate) fn status_shelf_uri(did: &str, status: ReadingStatus) -> String {
    format!("at://{did}/{SHELF_COLLECTION}/{}", status.as_str())
}

/// The rkey of an `at://` record URI, or `None` when the URI is malformed.
pub(crate) fn derive_rkey(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix("at://")?;
    let mut parts = rest.splitn(3, '/');
    let repo = parts.next()?;
    let collection = parts.next()?;
    let rkey = parts.next()?;
    if repo.is_empty() || collection.is_empty() || rkey.is_empty() {
        return None;
    }
    Some(rkey)
}

/// The body of a shelf record. `name` is the human label for the status.
pub(crate) fn shelf_record(status: ReadingStatus, now: &str) -> Value {
    json!({
        "$type": SHELF_COLLECTION,
        "name": status.shelf_name(),
        "createdAt": now,
        "updatedAt": now,
    })
}

/// The body of a shelving record, placing `book_uri` on `shelf_uri`.
pub(crate) fn book_shelving_record(
    shelf_uri: &str,
    book_uri: &str,
    title: &str,
    status: ReadingStatus,
    now: &str,
) -> Value {
    json!({
        "$type": BOOK_SHELVING_COLLECTION,
        "shelf": shelf_uri,
        "book": { "ref": book_uri, "title": title },
        "metadata": { "status": status.as_str() },
        "createdAt": now,
        "updatedAt": now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DID: &str = "did:plc:e2ctbutx6kya6si4if5ngjmm";
    const NOW: &str = "2026-10-24T23:54:31.707Z";

    /// Shape of the shelf records already in the account's repository
    /// (`com.atproto.repo.listRecords`, collection `…biblio.shelf`).
    #[test]
    fn shelf_record_matches_the_stored_records() {
        assert_eq!(
            shelf_record(ReadingStatus::ToRead, NOW),
            json!({
                "$type": "net.olamaelcu.livtet.biblio.shelf",
                "name": "Want to Read",
                "createdAt": NOW,
                "updatedAt": NOW,
            })
        );
        assert_eq!(
            shelf_record(ReadingStatus::Reading, NOW)["name"],
            "Currently Reading"
        );
        assert_eq!(shelf_record(ReadingStatus::Read, NOW)["name"], "Finished");
        assert_eq!(shelf_record(ReadingStatus::Dnf, NOW)["name"], "Abandoned");
    }

    /// Shape of a stored shelving record (`…bookShelving/ol.OL997340M`).
    #[test]
    fn book_shelving_record_matches_the_stored_record() {
        let book_uri =
            "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M";
        assert_eq!(
            book_shelving_record(
                &status_shelf_uri(DID, ReadingStatus::ToRead),
                book_uri,
                "Whores and other feminists",
                ReadingStatus::ToRead,
                NOW,
            ),
            json!({
                "$type": "net.olamaelcu.livtet.biblio.bookShelving",
                "shelf": "at://did:plc:e2ctbutx6kya6si4if5ngjmm/net.olamaelcu.livtet.biblio.shelf/to-read",
                "book": {
                    "ref": "at://did:web:biblio.livtet.olamaelcu.net/community.lexicon.book.edition/ol.OL997340M",
                    "title": "Whores and other feminists",
                },
                "metadata": { "status": "to-read" },
                "createdAt": NOW,
                "updatedAt": NOW,
            })
        );
    }

    #[test]
    fn status_shelf_uri_is_the_shelf_record_uri() {
        assert_eq!(
            status_shelf_uri(DID, ReadingStatus::Dnf),
            format!("at://{DID}/net.olamaelcu.livtet.biblio.shelf/dnf")
        );
    }

    #[test]
    fn derive_rkey_reads_the_last_segment() {
        assert_eq!(
            derive_rkey("at://did:plc:abc/net.olamaelcu.livtet.biblio.bookShelving/ol.OL997340M"),
            Some("ol.OL997340M")
        );
        assert_eq!(derive_rkey("not-a-uri"), None);
        assert_eq!(derive_rkey("at://did:plc:abc/collection"), None);
        assert_eq!(derive_rkey("at://did:plc:abc/collection/"), None);
    }
}
