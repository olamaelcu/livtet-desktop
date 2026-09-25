# livtet-epub

EPUB parsing and metadata extraction for Livtet imports.

Reads the OCF container, the OPF package document, and `META-INF/encryption.xml`
with a tolerant in-crate parser (no third-party EPUB library) and enforces
import requirements: every extracted record carries a non-empty title, at
least one contributor, and at least one validated ISBN-13.

Contributors from `dc:creator` and `dc:contributor` are de-duplicated by
`(name, role)`; a role-less contributor that merely repeats a creator is
dropped. `title_sort` is taken from the main title's `file-as` refinement or
`calibre:title_sort`.

```rust,ignore
let meta = livtet_epub::read_metadata(path)?;
meta.title;       // non-empty
meta.title_sort;  // Option<String>, from file-as / calibre:title_sort
meta.creators;    // ≥1 contributor, de-duplicated by (name, role)
meta.isbns;       // ≥1 validated ISBN-13 (ISBN-10 converted, Sigil prefixes handled)
```

Extraction fails closed: unreadable files, missing titles/creators, and
missing/invalid ISBNs are errors, never partial imports.
