# 1. EPUB import pipeline: `livtet-epub` extraction crate and `import_epub` command

Date: 2026-09-20

## Status

Accepted; partially superseded by [0011](0011-importer-plugin-contract-and-import-file-command.md)
for the direct `livtet-epub`/`import_epub` wiring. The decision to delegate
parsing to the `epub` crate is superseded by
[0018](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md).

## Context

Livtet needs to import books from EPUB files on disk. The import must produce
a complete catalog record (work, edition, contributors, publisher, subjects,
identifiers) and a `digital_inventory` row, and it must fail closed: a book
with no valid ISBN is not importable, because ISBNs anchor dedup,
reconciliation, and search.

Earlier prototypes considered parsing EPUB manually with `zip` + `quick-xml`.
The `epub` crate (v2.1) already validates the mimetype/container/OPF chain
internally, which eliminates a hand-rolled parsing layer. ISBN-10→13
upgrade, prefix stripping, and checksum validation already live in
`livtet_types::Isbn` in `../core`; duplicating that logic in a new crate
would create a second source of truth for ISBN correctness.

The desktop app previously consumed livtet core crates from a git URL
(`github.com/olamaelcu/livtet`, branch `main`), which made local iteration on
core+desktop together impossible and produced lockfile churn on every core
change.

## Decision

1. **New workspace member `livtet-epub`** (`desktop/crates/livtet-epub`)
   owns EPUB parsing. It depends only on `epub`, `thiserror`, and
   `livtet-types`. EPUB-specific identifier quirks — a `urn:` prefix and the
   Sigil convention of gluing a letter onto numeric NCName ids
   (`a9781784780609`) — are normalized in `livtet-epub`; validation is
   delegated to `livtet_types::Isbn`. The crate fails closed: unreadable
   file, missing title, missing creator, or no valid ISBN are all errors.

2. **Desktop consumes core by path**: the sibling checkout at
   `../core` (flat workspace: `livtet-core`, `livtet-data`, `livtet-search`,
   `livtet-types`) replaces all git-pinned livtet dependencies, declared once
   in `desktop/Cargo.toml`'s `[workspace.dependencies]`.

3. **Import is a single Tauri command `import_epub`** in
   `crates/livtet-desktop/src/commands/import.rs`. It hashes the file
   (SHA-256) for dedup against `digital_inventory.file_hash`, then writes
   works → editions → authors/publishers/subjects + junctions → identifiers
   → `digital_inventory` in one transaction. Format is the seeded
   deterministic `KnownFormats::Epub` id; language resolves via
   `CommonLanguages::normalize_language_code`. Identifier values are stored
   as `urn:isbn:<isbn13>` with kind `isbn`.

4. **Cover bytes write post-commit** to `<data>/covers/<edition_id>.<ext>`;
   a failed write nulls the column but never rolls back the import (the DB
   record is authoritative, the file re-derivable).

5. **AppState carries the database pool** (`db: SharedState`) alongside the
   search index, plus a `covers_dir` path, instead of using the global
   `init_state`/`get_state` singleton core provides.

6. **After a successful import**, `SearchIndex::add_edition` refreshes the
   index (currently a full reindex; the seam exists in livtet-search's write
   path for a future single-document upsert).

## Consequences

**Easier**: importing EPUBs is one IPC call; data contracts are the shared
`livtet_types` types (`Isbn`, `Urn`, `IdentifierKind`, `KnownFormats`), so
core and desktop can never disagree on ISBN validity; local edits to core
are picked up immediately without pushing to git.

**Harder**: the desktop app can no longer build standalone against a pinned
core revision — `../core` must be present and in sync. A full reindex per
import is acceptable at library-scale import rates but visible if bulk
import is added later; the `add_edition` seam should then be swapped for a
true upsert. `ImportError` flattens foreign errors to `(code, message)` so
Specta never exports core error types.

**Follow-ups**: a file-import UI and picker binding `import_epub`; possible
bulk-import path; `add_edition` single-document write path.
