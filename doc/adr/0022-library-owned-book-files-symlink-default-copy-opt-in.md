# 22. Library-owned book files: symlink default, copy opt-in

Date: 2026-09-25

## Status

Accepted.

## Context

`digital_inventory.file_path` stores the user's original absolute import path,
and `delete_editions` (`crates/livtet-desktop/src/commands/bulk.rs`) unlinks
that file. Removing a book from the library therefore destroys the user's
download. The library needs its own reference to every imported file so that
deleting the library entry never touches the source, while avoiding a second
full copy of every book on disk by default.

## Decision

1. Import materializes a library-owned file under a new `books_dir`,
   `{app_dir}/data/books` (sibling of `{app_dir}/data/covers`), held on
   `Paths` and `AppState` as `books_dir`.
2. The store name is `{sha256_hex}-{original_filename}`: the content hash the
   importer already computes (first-seen dedup order is unchanged), plus the
   source's `file_name()` so the entry stays human-readable on disk. The name
   is length-capped for `NAME_MAX`; an empty source name falls back to
   `book.{ext}`. This path — not the source — is stored in
   `digital_inventory.file_path`. No schema change.
3. The default storage mode is a symlink to the source file (`ImportMode::Link`).
   If symlink creation fails with `PermissionDenied` or `Unsupported` (default
   Windows), fall back to a full byte copy with a warning log; other errors
   fail the import (`io`). Materialization happens after hash dedup and before
   the catalog transaction; a transaction error removes the materialized file
   (fail-closed, no orphan).
4. A per-batch `ImportMode` parameter on `import_file` / `import_files` selects
   the mode. The add-book drawer exposes it as a "Copy files into library"
   switch (default off); a duplicate import (hash match) materializes nothing
   regardless of mode.
5. `delete_editions` is unchanged: it unlinks `file_path`, which for new rows
   is the library-owned link/copy. Rows imported before this change still point
   at user originals and keep the old behavior (no migration, no guard).
6. `EditionFile` gains `file_status: Ok | Missing` (null when no path), computed
   from target existence at `get_edition_detail` time. The detail drawer shows a
   missing-file warning with a re-link action.
7. `relink_edition_file(edition_id, path)` repoints a library file at a newly
   picked source: it accepts the file and updates the edition's file identity
   (`file_path`, `file_hash`, `file_size_bytes`, `file_format`), preserving the
   entry's existing storage kind (symlink stays symlink). Only a previous file
   inside `books_dir` is removed; legacy source paths are never deleted.

## Consequences

### Becomes easier

- Deleting a book from the library is non-destructive to the user's files.
- The common case costs no extra disk; the opt-in copy covers sources that may
  disappear (removable media, temp downloads).
- A dangling link is a visible, recoverable state instead of silent breakage.

### Becomes harder or carries risk

- Symlink targets are live: editing the source edits the library's bytes, and
  moving/deleting the source dangles the link until re-linked.
- Same-filesystem storage is assumed for links; cross-device sources silently
  become copies via the fallback, which costs disk without an explicit user
  choice at that moment.
- `file_hash` no longer implies "bytes at file_path are the imported bytes"
  after a re-link — it identifies the currently linked file.
