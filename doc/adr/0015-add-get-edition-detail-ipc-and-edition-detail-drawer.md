# 15. Add get_edition_detail IPC and edition-detail drawer

Date: 2026-09-24

## Status

Accepted

## Context

- The library route (`web/routes/library/+page.svelte`) lists editions from the
  Tantivy search index through `search_editions`. The index carries
  title/authors/pub_date/format/language/ISBN, but not cover, publisher, or
  notes, and the desktop client only surfaced title/authors/published/snippet.
- Hovering a book showed a `wa-popover` with those four fields; clicking did
  nothing.
- ADR 0005 built a catalog detail surface (`find_files_by_edition`,
  `find_identifiers_by_edition`, `find_authors_by_edition`, `web/lib/catalog/`,
  a peek state, and a `/catalog/[editionId]` route). That work was dropped in
  the branch hard-reset: no detail command or `web/lib/catalog/` remains.
- ADR 0008 began populating `digital_inventory.cover_path` on import and
  deferred cover serving to a separate ADR. `assetProtocol.enable` was true but
  had no `scope`, so no `asset:` URL could load.
- `specta-typescript` refuses `i64`/`u64`/`usize`, and
  `digital_inventory.file_size_bytes` is `i64`.

## Decision

### 1. One aggregate `get_edition_detail` command

`crates/livtet-desktop/src/commands/catalog.rs` adds a single read-only command:

```
get_edition_detail(editionId: string) -> EditionDetail | null
```

It parses the id to `DbId` (fail-closed on invalid input) and returns `null`
when the edition is absent. Assembly mirrors `livtet-ffi`'s `edition_detail()`
(the FFI surface is not reused): `editions` plus `formats`/`languages`,
`edition_authors` → `authors` carrying the junction `role`, `edition_publishers`
→ `publishers`, `edition_identifiers` → `identifiers`, and `digital_inventory`.
A new `CatalogError { InvalidId, Database }` carries failures.

The query logic lives in a `fetch_edition_detail(&DatabaseConnection, DbId)`
free function so it is unit-testable against `livtet_core::data::TestDb`.

### 2. Types are specta-shaped

`EditionDetail`, `Contributor`, `IdentifierRef`, and `EditionFile` are
`Serialize + Type` DTOs. `file_size_bytes` crosses as `Option<f64>` (specta
cannot export `i64`); values ≤ 2^53 round-trip exactly. `published_date` is
rendered `YYYY-MM-DD`.

### 3. Cover serving via the asset protocol

`tauri.conf.json` gains
`app.security.assetProtocol.scope = ["$APPDATA/data/covers/**/*"]` (the CSP
already allows `asset:` in `img-src`). Covers live under
`<app-data-dir>/data/covers/<inventory_id>/cover.<ext>` because `Paths::new`
nests `data/` under `livtet_core::paths::data_dir()`. The frontend turns
`EditionFile.cover_path` into a URL with `convertFileSrc`; a missing or failed
image falls back to the letter placeholder.

### 4. Frontend: the drawer replaces the hover popover

- `web/lib/library/EditionDetailDrawer.svelte` is a right-hand `wa-drawer`
  backed by a TanStack query keyed on `catalogKeys.editionDetail(id)`.
- `BookCard.svelte` moves to Svelte 5 props and takes an `onclick`. The library
  page removes the `wa-popover`, its Escape hotkey, and the associated styles,
  and filters `books` to `kind === "edition"` (already guaranteed by
  `search_with_options`, kept as a client guard).
- `mapHitToEdition` now carries `edition_id` and `kind`.

## Consequences

- Every "show the full record for edition X" need is one IPC call and one
  binding; adding fields is a DTO edit plus binding regeneration.
- The drawer depends on the cover existing at the stored absolute path. If
  `$APPDATA` does not resolve to `dirs::data_dir()/<bundle-id>` on some platform,
  the scope must move to a runtime `asset_protocol_scope().allow_directory()`;
  verify on Windows in particular.
- Covers remain unsurfaced in the list (`mapHitToEdition` still leaves
  `cover_url` undefined); that needs a batch lookup and is out of scope.
- `file_size_bytes` is a JS `number` now, not an integer; documented on the
  field.
- ADR 0005's multi-command catalog surface is not resurrected; this is the lean
  replacement.

## Links

* [5. Pin digital_inventory edition_id UNIQUE; extend tauri-specta edition-detail IPC](0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md)
* [8. Hash-keyed file cover storage with provider-backed fetchers](0008-hash-keyed-file-cover-storage-with-provider-fetchers.md)
