# livtet-epub

EPUB parsing and metadata extraction for Livtet imports.

Reads the OCF container, the OPF package document, and `META-INF/encryption.xml`
with a tolerant in-crate parser (no third-party EPUB library) and enforces the
import minimum: every extracted record carries a non-empty title and at least
one contributor. An ISBN is optional: when the metadata declares none, the
content documents are scanned (spine order, then copyright/title-page hrefs) for
a checksum-valid ISBN, bounded to 10 documents, 2 MiB each, and 16 MiB total, so
`isbns` may legitimately be empty. Non-ISBN identifiers (UUID, ASIN, publisher
ids) are preserved in `other_identifiers` so an edition without an ISBN still
has identity.

Contributors from `dc:creator` and `dc:contributor` are de-duplicated by
`(name, role)`; a role-less contributor that merely repeats a creator is
dropped. `title_sort` is taken from the main title's `file-as` refinement or
`calibre:title_sort`.

```rust,ignore
let meta = livtet_epub::read_metadata(path)?;
meta.title;               // non-empty
meta.title_sort;          // Option<String>, from file-as / calibre:title_sort
meta.creators;            // ≥1 contributor, de-duplicated by (name, role)
meta.isbns;               // validated ISBN-13s; may be empty
meta.other_identifiers;   // non-ISBN identifiers (UUID/ASIN/…), preserved verbatim
```

Extraction fails closed: unreadable files and missing titles or contributors are
errors, never partial imports. A missing ISBN is not an error.
