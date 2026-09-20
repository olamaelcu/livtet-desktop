# livtet-epub

EPUB parsing and metadata extraction for Livtet imports.

Layers Livtet's normalization on top of the [`epub`](https://docs.rs/epub)
crate and enforces import requirements: every extracted record carries a
non-empty title and at least one validated ISBN-13.

```rust,ignore
let meta = livtet_epub::read_metadata(path)?;
meta.title;    // non-empty
meta.creators; // ≥1 contributor with MARC-relator role
meta.isbns;    // ≥1 validated ISBN-13 (ISBN-10 converted, Sigil prefixes handled)
```

Extraction fails closed: unreadable files, missing titles/creators, and
missing/invalid ISBNs are errors, never partial imports.
