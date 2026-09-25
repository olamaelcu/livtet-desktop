# 20. ISBN optional for EPUB imports with a body-text fallback

Date: 2026-09-24

## Status

Accepted; supersedes the ISBN-requirement portion of
[0018](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md) and
[0009](0009-epub-import-pipeline.md).

## Context

Real books in the user's library — Gutenberg texts, self-published works, and
store purchases — often carry no ISBN in their OPF metadata. They identify the
work with a UUID or ASIN, or the ISBN appears only in the running text of the
copyright page rather than in `dc:identifier`. The previous fail-closed rule
required at least one validated ISBN-13, which blocked 5 of 8 EPUBs in a
representative library even though each carried a title and contributors.

## Decision

1. An EPUB import requires only a non-empty title and at least one contributor.
   ISBNs are optional, so `isbns` may legitimately be empty.
2. When the OPF metadata declares no ISBN, scan the content documents for the
   first checksum-valid ISBN: spine order first, then any href that looks like a
   copyright or title page, then the remaining content documents. The scan is
   bounded to `BODY_SCAN_MAX_DOCUMENTS` (10 documents),
   `BODY_SCAN_MAX_DOCUMENT_BYTES` (2 MiB per document), and
   `BODY_SCAN_MAX_TOTAL_BYTES` (16 MiB total).
3. Persist non-ISBN identifiers (UUID, ASIN, other) as `identifiers` /
   `edition_identifiers` rows so a no-ISBN edition still has identity, and
   demote any unparseable ISBN entry to a non-ISBN identifier instead of
   failing the import.
4. Import de-duplication remains file-hash based and is unaffected by the
   identifier change.

## Consequences

**Easier**: UUID/ASIN-only and body-ISBN books import; no edition is rejected
merely for lacking an ISBN; identity is preserved through the identifier rows.

**Harder**: imported editions can now lack an ISBN, so catalog matching keyed on
ISBN is best-effort rather than guaranteed; the body scan is a bounded heuristic
that can miss an ISBN split across markup or buried past the document and byte
caps.

## Links

* [9. EPUB import pipeline](0009-epub-import-pipeline.md)
* [18. Replace the epub crate with an in-crate OCF/OPF parser](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md)
