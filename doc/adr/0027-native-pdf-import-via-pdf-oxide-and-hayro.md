# 27. Native PDF import via pdf_oxide and hayro

Date: 2026-09-25

## Status

Accepted.

## Context

The importer contract
([ADR 0011](0011-importer-plugin-contract-and-import-file-command.md)) routes
only `.epub` — and, since
[ADR 0023](0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md),
the MOBI family — to a native parser; every other extension falls through to
the remote Lua path. That path has no PDF plugin, so `.pdf` import did not work
at all.

PDF bibliographic data is split across two carriers that disagree: the trailer
`/Info` dictionary (`Title`, `Author`, `Subject`, `Keywords`, `CreationDate`)
and an optional XMP packet (Dublin Core plus `pdf:` and custom properties). A
cover is either an embedded image XObject or a render of page 0.

No single maintained crate covers parse + Info + XMP + image extraction + PNG
encoding without a large rendering stack, and hand-rolling a PDF parser in the
spirit of
[ADR 0018](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md) is
not proportionate: the format is far larger than EPUB's OCF/OPF and the
metadata surfaces are well-served by an existing parser.

## Decision

1. **`pdf_oxide` for all-in-one extraction**, pinned to exactly `=0.3.78`
   (MIT OR Apache-2.0). It exposes the Info dictionary (`DocumentEditor` +
   `EditableDocument::get_info`), XMP (`XmpExtractor`), and embedded images
   (`page_image_handles`, with JPEG passthrough via `raw_compressed_bytes` and
   re-encoding via `PdfImage::to_png_bytes`) under its default features and
   without the `rendering` feature.
   * `office_oxide` is held at `=0.1.9` in `Cargo.lock`. `pdf_oxide` 0.3.78
     declares the open range `office_oxide = "0.1.9"`, but 0.1.10+ added
     `DocumentIR::defined_names`, which 0.3.78 does not initialize, so 0.1.12
     does not compile as its dependency. The lock pin keeps the build whole and
     is load-bearing; a blanket `cargo update` will break it.
2. **`hayro` 0.7 for the cover fallback only** (`#![forbid(unsafe_code)]`,
   Apache-2.0 OR MIT). It renders page 0 to a `Pixmap` that is encoded to PNG.
   Metadata and embedded-image extraction do not depend on it.
3. **Fail-closed semantics.** An unreadable, malformed, or encrypted document,
   and a document missing a title or a creator, is an error — mirroring the
   EPUB and MOBI parsers. An ISBN and a cover are optional.
4. **Cover: embedded first, render second.** Take the largest image XObject
   painted on page 0; pass `DCTDecode` (JPEG) bytes through untouched and
   re-encode everything else to PNG. Only when that yields nothing, render page
   0 to a white-backed PNG at roughly 600 px wide. A cover failure is never an
   error and never returns ciphertext.
5. **ISBN recovery is metadata-only.** Scan Info `/Keywords` and `/Subject`,
   and XMP `pdf:Keywords`, `dc:subject`, and custom values, validating with
   `livtet_types::Isbn`. Page text is never extracted. Non-ISBN candidates are
   preserved as `other_identifiers`.
6. **Title and creators are XMP-first, then Info.** An Info author string is
   split on ` and ` / ` with ` / ` & `, with `&&` an escaped literal `&`; `;`
   is not a separator.

## Consequences

**Easier**: `.pdf` now imports through the same pipeline as `.epub` and the
MOBI family, with no remote plugin; covers come from the file itself; no PDF
parsing code is owned in-tree.

**Harder / accepted limitations**: `publisher` is always absent (PDF has no
standard publisher field); `published` is the document creation date — the
file's date, not necessarily the work's publication date; an ISBN is only found
in metadata keywords, never in a page's running text; encrypted PDFs are
rejected with no password path. `pdf_oxide` brings a large default dependency
tree (`image`, `quick-xml`, `nom`, `office_oxide`, …) whose compile time and
supply-chain surface are real, and the two exact pins above are load-bearing.
Revisit if a smaller, permissively licensed PDF metadata crate appears, or when
`pdf_oxide` ships a release whose `office_oxide` range is self-consistent.

## Links

* [11. Importer plugin contract and import_file command](0011-importer-plugin-contract-and-import-file-command.md)
* [18. Replace the epub crate with an in-crate OCF/OPF parser](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md)
* [23. Import MOBI-family files with an in-crate livtet-mobi parser](0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md)
