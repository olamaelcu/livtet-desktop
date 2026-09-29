# 32. Calibre library import via a bundled Lua plugin

Date: 2026-09-29

## Status

Accepted. Builds on [ADR-0031](0031-host-provided-sandbox-modules.md).

## Context

Users migrating from Calibre want to bring their library into Livtet. A Calibre
library is a folder of book directories plus a `metadata.db` SQLite database that
holds the authoritative metadata (authors, tags, identifiers, series, formats).

## Decision

Add a `LibraryImporter` contract to `livtet-importer` — `scan(source) ->
[BookRecord]`, where a `BookRecord` bundles an `ImporterMeta` with its format
files and an optional cover, all relative to the library root — and ship a
first-party **Calibre plugin** that implements it by querying `metadata.db`
through the `sqlite` module (ADR-0031).

The `import_calibre_library` command embeds the plugin with `include_str!` and
materializes it to a temp directory per import, so it needs no separate
installation. It launches the host with a read-only `--sqlite` grant on the
library's `metadata.db`, calls `scan`, then persists each record through the
shared `persist_import` path (reused from `import.rs`), reading covers from disk
and adding new editions to the search index. An "Import from Calibre…" action in
the add-book drawer drives it.

## Consequences

- Easier: a Calibre library imports with its real metadata (not re-extracted
  from files), and the flow dogfoods the `sqlite` module + `LibraryImporter`
  contract for future library importers.
- Limits: each format file becomes its own edition (matching the per-file import
  model); series is dropped (no field in `ImporterMeta` yet); multi-value fields
  beyond the first (e.g. multiple languages) are not modeled. These are known
  simplifications, not blockers.
