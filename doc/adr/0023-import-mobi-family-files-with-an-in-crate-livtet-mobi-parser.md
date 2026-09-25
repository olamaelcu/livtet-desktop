# 23. Import MOBI-family files with an in-crate `livtet-mobi` parser

Date: 2026-09-25

## Status

Accepted.

## Context

The importer contract ([ADR 0011](0011-importer-plugin-contract-and-import-file-command.md))
routes only `.epub` to a native parser; every other extension falls through to
the remote Lua path. Amazon Kindle files (`.azw3` = KF8, `.azw` = the older
MOBI-family container) had no importer at all.

Candidate crates were rejected on the same grounds as the EPUB decision
([ADR 0018](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md)):
`mobi` 0.8 (MIT) is stale and MOBI6-oriented, hides the EXTH and cover details
the importer needs, and still requires custom cover logic; `ebook-rs` is a
full multi-format engine (tree-sitter, readium) that dwarfs a metadata-only
need.

A 20-file Kindle corpus (19 `.azw3`, 1 `.azw`) established the shape of the
problem: every file is a PalmDB container whose record 0 carries a PalmDOC
header, a MOBI header, and EXTH metadata; **17 of 20 carry Amazon DRM
(`encryption_type = 2`) yet their EXTH and cover records are plaintext**;
all KF8 files use HUFF/CDIC text compression; EXTH 108 and some `full_name`
values are HTML-entity-encoded while EXTH 100 is raw UTF-8; EXTH 503 disagrees
with `full_name` in 13/20; `EXTH` 121 appears in 10 files without an actual
`BOUNDARY` record; and EXTH 103 (description) and 105 (subjects) are absent
throughout.

## Decision

1. Add `crates/livtet-mobi`, parsing the PalmDB record table, the PalmDOC/MOBI
   headers, and the EXTH block by hand. Its only dependencies are
   `livtet-importer-types`, `livtet-types`, and `thiserror` — no third-party
   parsing crate. Reject KFX (`\xeaDRMION\xee`) and Topaz (`TPZ`) as
   `UnsupportedContainer`; accept `BOOKMOBI` and `TEXTREAD`.
2. Extract **metadata and cover only**. No PalmDOC/HUFF decompression, no
   SKEL/FRAG or HTML reconstruction. The importer contract returns a
   bibliographic record, not a readable book.
3. **Import DRM'd files.** Only the text records are encrypted; EXTH and image
   records are plaintext, so `encryption_type` is read for information and never
   rejects a file. Rejecting DRM would have excluded 17 of the 20 corpus files.
4. Metadata extraction rules: prefer EXTH 503 over `full_name` (often a slug or
   truncated); HTML-entity-decode string fields; map `Last, First` to
   `name`/`file_as` while preserving parenthesised surnames verbatim; treat
   whitespace-only publishers as absent; canonicalize ISBNs with `Isbn::parse`
   after stripping `eBook ISBN:` and `urn:isbn:` prefixes, demoting the rest to
   other identifiers; split subjects on `;`; resolve language from EXTH 524,
   falling back to the numeric locale (`9`/`1033` → `en`).
5. Resolve the cover as EXTH 201 → 202 → first image-magic record on or after
   `first_image_index`, sniffing JPEG/PNG/GIF/BMP/WEBP and never returning a
   non-image resource.
6. Bound every read and fail closed: use the PDB `nrec` (not the PalmDOC
   `record_count`, which omits image/index records), tolerate EXTH padding and
   extra `exth_flags` bits, skip a malformed EXTH item rather than discarding
   the block, and return `MobiError` instead of panicking on truncation.
7. Extract the parser-side model shared by `livtet-epub` and `livtet-mobi` into
   `crates/livtet-importer-types` (`SourceMetadata` plus the newtypes, `Role`,
   `Contributor`, `PublicationDate`, `Cover`). `livtet-importer` maps
   `SourceMetadata` onto the unchanged Lua wire `ImporterMeta`.
8. Register a distinct `KnownFormats::Azw3 = 9` in the core repo (KF8 is not
   MOBI6), backfilled by core migration `m0010_azw3_format`. Desktop routes
   `.azw3` → `Azw3` and `.azw` → `Mobi`, both to the native `MobiImporter`.
   `.mobi` is deliberately not registered.

## Consequences

### Becomes easier

- Kindle libraries import natively: title, contributors, publisher, ISBNs,
  ASINs, language, date, and cover, with no remote plugin round-trip.
- DRM'd purchases are catalogued (metadata + cover) even though their text is
  not yet readable — the catalogue is honest about what it holds.
- Both native importers return one shared model, so a third parser does not
  duplicate the metadata types.

### Becomes harder or carries risk

- DRM'd or HUFF/CDIC files cannot be read in-app; reading requires a decryptor
  and text decompression, both out of scope here.
- The PalmDB/MOBI/EXTH layout is hand-maintained knowledge; a rare header
  variant may need a follow-up rather than a crate upgrade.
- The entity/encoding rules and the `.azw`-as-MOBI6 path are covered only by
  synthetic fixtures, since the local corpus is single-codepage.
- Adding the format required a core-repo release followed by a desktop
  `cargo update`, coupling the two changes across repositories.
