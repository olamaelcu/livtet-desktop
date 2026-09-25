# 21. File-availability filter and card badges

Date: 2026-09-24

## Status

Accepted

## Context

The library listed editions without any signal of whether a file exists on
disk. The search index already returned `SearchResult.has_file` (derived from
`digital_inventory`), but `mapHitToEdition` dropped it, and no filter could
separate on-disk editions from virtual or remotely-referenced ones (for
example an OPDS collection), which is the groundwork for provenance filtering.

## Decision

1. Surface `has_file` on the `Edition` view model and render a paired badge on
   `BookCard`: `hard-drive` when the edition is on disk, `cloud` when it is
   not.
2. Add `has_file: Option<bool>` to the desktop `EditionFilters` DTO and map it
   into core `WorkFilters.has_file` (see [core ADR 3](../../core/docs/adr/0003-index-file-availability-as-a-filterable-search-field.md)).
   `None` = any, `Some(true)` = in filesystem, `Some(false)` = not in
   filesystem.
3. Extend `filterAxes` with a tri-state availability filter: `setHasFile`,
   counted by `activeFilterCount`, emitted as a chip (and cleared) like an axis.
   `false` is a real constraint and must never be normalised away.
4. Expose an **Availability** select in the filter panel. Filtering is
   server-side through the index, so counts, pagination, and select-all
   (`matching_edition_ids`) stay correct.

## Consequences

**Easier**: users can find or exclude local files, and the boolean is the seam
for future provenance (OPDS, plugin sources). One filtered+sorted search path
covers the card list, counts, and bulk selection.

**Harder**: desktop depends on a newer core commit (search schema v4, which
reindexes once on next launch) and must regenerate `bindings.ts`.
