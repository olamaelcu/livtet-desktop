# 35. Book conversion with EPUB as the root format

Date: 2026-10-04

## Status

Pending

## Context

We need a conversion pipeline between book formats (PDF, MOBI, EPUB, etc.) for the import/export flow. Earlier exploration considered a generic `Converter` trait with `SourceMetadata` → `ImporterMeta` conversion. After reviewing the existing `livtet-importer` typed contract (`PdfImporter`, `EpubImporter`, etc.) and the format-specific parser crates (`livtet-pdf`, `livtet-epub`), it's clear that a single generic converter is the wrong abstraction.

The user specified EPUB as the root format: other formats convert in and out of EPUB rather than through a bilateral trait pair.

## Decision

(Not yet accepted; under discussion)

1. **EPUB is the canonical format.** `EpubDocument` (in `livtet-converter-types`) represents the internal conversion state.
2. **Two-direction traits per format crate:** `IntoEpubDocument` (format → epub) and `FromEpubDocument` (epub → format) implemented in `livtet-pdf`, `livtet-mobi`, etc.
3. **New crates:** `livtet-converter-types` (types/traits) and `livtet-converter` (orchestration / registry).
4. **Integration test:** ePub → PDF → back via `livtet-pdf` trait implementations; verify `ImporterMeta` equality through `livtet-importer` contract.
5. **Constraints:** `unwrap_used = "deny"`, `expect_used = "deny"`; edition 2024; rust-version 1.97.0.

## Consequences

If adopted: format crates become bidirectional conversion points; EPUB stays the single source of truth; no `Converter` trait needed. If rejected: revert to generic trait or direct format-to-format mappings.

## Related

- ADR 0011 (importer plugin contract)
- ADR 0027 (native PDF import via pdf-oxide)
- ADR 0009 (epub import pipeline)
