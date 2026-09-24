# 17. Bulk edition mutations

Date: 2026-09-24

## Status

Accepted

## Context

The library had no multi-select or batch operations. Deleting an edition must
remove its catalog rows, its file and covers, and its index document; the
on-disk layout is `covers_dir/<inventory_id>/cover.<ext>` and manual covers live
in `edition_specific_covers`.

## Decision

1. Bulk actions operate on edition IDs and tag editions only (`edition_tags`).
2. `delete_editions` captures inventory/cover paths, deletes the editions
   (junctions, `digital_inventory`, and `edition_specific_covers` cascade), then
   best-effort unlinks files and removes the index documents. Orphaned works are
   left in place.
3. `export_editions_csv` writes `<path>.tmp` then renames, so a failure leaves no
   partial file. Columns: edition_id, work_id, title, authors, isbn, publisher,
   language, format, published_date.
4. `add_edition_tags` / `remove_edition_tags` find-or-create the tag by trimmed
   name, write `edition_tags`, and rebuild the index once.
5. `matching_edition_ids` resolves select-all server-side via
   `matching_work_ids_from_query`, capped at `OPDS_WORK_ID_LIMIT` (1000 works).
6. Errors use a flat `{ code, message }` type named `BulkError` (renamed from a
   proposed `CatalogError` to avoid a specta name collision with the existing
   catalog error type).

## Consequences

**Easier**: one delete path that keeps DB, disk, and index consistent; bulk
tagging reuses the filter option list.

**Harder**: deletes are irreversible (no trash); the select-all cap must be
surfaced; file removal is best-effort and reported via `skipped`.
