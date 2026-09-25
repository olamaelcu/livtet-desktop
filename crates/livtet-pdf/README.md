# livtet-pdf

PDF metadata and cover extraction for Livtet imports.

## Scope

`read_metadata(path) -> Result<PdfMetadata>` reads a PDF's Info dictionary and
XMP packet and returns one source record. It does **not** extract text or
render page content beyond the cover fallback.

* **Title / creators**: XMP first (`dc:title`, `dc:creator`), then the Info
  dictionary (`/Title`, `/Author`). An Info author string is split on
  ` and ` / ` with ` / ` & `; `&&` is an escaped literal `&`, and `;` is not a
  separator.
* **ISBNs**: optional. Recovered only from metadata — Info `/Keywords` and
  `/Subject`, and XMP `pdf:Keywords`, `dc:subject`, and custom properties —
  validated with `livtet_types::Isbn`. Page text is never scanned.
* **Non-ISBN identifiers**: every non-empty candidate that is not an ISBN is
  preserved verbatim so an edition keeps identity.
* **Cover**: the largest embedded image painted on page 0, JPEG passed through
  byte-for-byte and everything else re-encoded to PNG; otherwise a white-backed
  render of page 0 scaled to roughly 600 px wide.
* **Publisher**: always absent — PDF has no standard publisher field.

## Fail-closed rules

An unreadable, malformed, or encrypted file, and a document with no title or no
creator, are errors. Everything else is best-effort: a missing cover or ISBN is
not an error.

## Known limitations

* `published` is the document creation date (XMP `xmp:CreateDate`, else Info
  `/CreationDate`) — the file's date, not necessarily the work's.
* An ISBN that appears only in the running text of a page is not recovered.
* Encrypted PDFs are rejected; there is no password path.
