# 16. Filtered library search IPC

Date: 2026-09-24

## Status

Accepted

## Context

`search_editions` exposed only free text and pagination, though the Tantivy index
supports per-axis `WorkFilters`. Sorting and filtering must compose, but
`SearchReader::search_with_query` ignored `SearchOptions.sort` while
`search_with_options` sorted yet built its query without caller filters.

## Decision

1. Add `EditionFilters` (a desktop DTO without `limit`, because `u64` breaks
   specta-typescript) mapped to `livtet_types::WorkFilters { limit: None, .. }`.
2. Resolve format/language `DbId`s to index labels via `LabelResolver`, build a
   `WorkFiltersQuery`, and search/count through `search_with_query` /
   `count_with_query`. `total` counts edition documents so it matches the hit
   stream, and relevance stays the default ordering (sort is opt-in).
3. Extend core `SearchReader::search_with_query` to honour `SearchOptions.sort`,
   sharing the sort/limit/offset tail with `search_with_options` via
   `finish_edition_search`; `SortField::Title` now reads the stored (lowercased)
   `title` because `title_sort` is not STORED.
4. Add `filter_options`, returning every value per axis sorted by label.

## Consequences

**Easier**: one filtered+sorted search path; the panel lists real values.

**Harder**: `filter_options` returns unbounded lists (revisit with facets or
server-side search for large author/subject sets); desktop depends on a newer
core commit.
