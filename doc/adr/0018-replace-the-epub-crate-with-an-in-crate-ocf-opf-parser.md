# 18. Replace the epub crate with an in-crate OCF/OPF parser

Date: 2026-09-24

## Status

Accepted; supersedes the `epub`-delegation decision in
[0009](0009-epub-import-pipeline.md). The inherited ISBN-requirement is
superseded by
[0020](0020-isbn-optional-for-epub-imports-with-a-body-text-fallback.md).

## Context

`livtet-epub` delegated container and package parsing to the `epub` 2.1 crate.
That crate is `GPL-3.0` — the only GPL dependency in an otherwise `MPL-2.0`
workspace — and it hid the raw OPF attributes the importer needs (MARC-relator
`role`, `file-as`, `calibre:title_sort`), made EPUB2 cover-by-href resolution
awkward, had no `META-INF/encryption.xml` awareness, and offered no recovery
for archives whose ZIP central directory is damaged.

A concrete import bug forced the issue: a `dc:creator` plus a role-less
same-name `dc:contributor` produced two identical author bindings, violating
the `edition_authors` unique key.

## Decision

1. Drop the `epub` dependency; parse OCF (`META-INF/container.xml`), the OPF
   package document, and `META-INF/encryption.xml` in-crate with `quick-xml`
   0.42 and `zip` 8.
2. De-duplicate contributors by `(name, role)`, preserving first-seen order
   after a stable `display-seq` sort. A role-less `dc:contributor` resolves to
   `ctb` and is dropped when it merely repeats a `dc:creator`.
3. Resolve the cover by precedence — EPUB3 `cover-image` property, then
   `<meta name="cover">` (manifest id, then href), then a `<guide>`
   `type="cover"` reference — with a media-type guard and an encryption skip so
   encrypted or XHTML items are never returned as image bytes.
4. Parse XML tolerantly (stray `&`, mixed encodings, illegal control
   characters, unclosed tags) with namespace-agnostic local-name matching, and
   fall back to scanning raw local file headers when the ZIP central directory
   is damaged.
5. Relicense `livtet-epub` to `MPL-2.0`.

## Consequences

**Easier**: the workspace is GPL-free; the importer reads the exact OPF
attributes it needs (roles, `file-as`, `calibre:title_sort`); imports no longer
collide on `edition_authors`; damaged and hostile archives are handled instead
of failing or exhausting memory.

**Harder**: we own the parser and must maintain it. Reads are bounded by
`MAX_ENTRY_BYTES` (64 MiB per entry) and `MAX_DEPTH` (512 nested elements) to
resist ZIP bombs and stack exhaustion, with two heavier zip-bomb tests. Revisit
if a well-maintained, permissively licensed EPUB crate appears.

## Links

* [9. EPUB import pipeline](0009-epub-import-pipeline.md)
