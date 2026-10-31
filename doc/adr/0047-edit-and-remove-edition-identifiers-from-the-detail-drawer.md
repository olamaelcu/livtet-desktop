# 47. Edit and remove an edition's identifiers from the detail drawer

Date: 2026-10-31

## Status

Accepted

## Context

- ADR 15 gave the edition-detail drawer a read-only Identifiers section:
  `get_edition_detail` returns `IdentifierRef { kind, value }` and the drawer
  renders one row per entry. Nothing in the app could correct a mistyped ISBN or
  drop an entry a metadata source invented; the only way out was deleting the
  book.
- `identifiers.value` is UNIQUE and `identifiers.kind` lives on that same row,
  while `edition_identifiers` is a junction with a composite
  `(edition_id, identifier_id)` key. An identifier row is therefore shared by
  every edition carrying the value, and the IPC's `IdentifierRef` carries no
  identifier id.
- The search index stores identifiers as `identifier_values` / `identifier_kinds`
  on every edition document, so a write here that skips reindexing leaves the
  library's ISBN search answering from a stale document.
- `livtet-ffi` offers `add_edition_identifier` only; it is not reachable from the
  desktop's IPC surface.

## Decision

### 1. Two commands, addressed by `(kind, value)`

`crates/livtet-desktop/src/commands/identifier_edit.rs` adds:

```
remove_edition_identifier(editionId, kind, value) -> null
update_edition_identifier(editionId, kind, value, newKind, newValue) -> null
```

`CatalogError` grows `InvalidIdentifier`, `IdentifierNotFound`, and
`IdentifierConflict`. Both commands parse the id fail-closed and resolve the
entry through the edition's own links, so a row another book holds is not this
edition's to edit — the pair `(kind, value)` is enough without exposing an
identifier id through the bindings.

### 2. Writes are link-first, and shared rows are never rewritten

An update runs in one transaction: unlink the old entry, delete the identifier
row if that left it orphaned, then find-or-create the new one and link it. A
rejected edit (blank field, kind clash) rolls back, leaving the edition exactly
as it was.

`kind` is a property of the shared row, so re-pointing at a value that another
book stores under a different kind fails with `IdentifierConflict` instead of
silently changing that book's identifier. Removal deletes the link, then the row
once nothing else points at it.

### 3. Reindexing is one document and best effort

Both commands rebuild only the touched edition's search document
(`SearchIndex::add_edition`, as `apply_enrichment` does) and log a failure
instead of failing the command: the catalog row is the source of truth, and a
missing index must not report a stored edit as rejected. This follows the
Calibre importer's best-effort indexing rather than enrichment's propagating
one.

### 4. A Kind / Value table, one dialog, one confirm

`EditionDetailDrawer.svelte` renders the identifiers as a `kind` / `value`
table (chosen over rows and chips after a side-by-side review): the scheme gets
a fixed column, values stay monospace and wrap, and each row carries an edit and
a remove action that are dimmed rather than hidden.

Clicking a value or its pencil opens `IdentifierEditDialog.svelte` inside a
`wa-dialog` (the pattern `ReviewInsert` already uses). The dialog edits the
*display* form (`formatIdentifier`) and rebuilds the stored string with
`composeIdentifier`, which puts `urn:<kind>:` back under the edited kind and
stores verbatim a value the reader typed out in full or that never was a URN.
Its own Delete needs no second confirmation: it is an explicit choice inside a
modal.

The row's trash button goes through `ConfirmDialog.svelte` first, because one
icon click is too cheap for an unrecoverable write. On any successful write the
drawer invalidates `catalogKeys.editionDetail` and `searchKeys.all`.

## Consequences

- Identifiers are correctable in place; the drawer section disappears on its own
  when the last entry goes, because it is gated on `book.identifiers.length`.
- The dialog is keyed on the pair, not an id, so editing a value to itself under
  a new kind works while a kind clash on a shared value is reported rather than
  guessed at.
- Orphaned `identifiers` rows are now cleaned up by the editor. Imports
  (`catalog_write::upsert_identifier`) still leave a row behind when a book is
  deleted, so orphans can predate this ADR.
- Duplicate `(kind, value)` entries on one edition cannot be created: the
  junction key and the value UNIQUE constraint forbid it.
- Search stays consistent because the edit path reindexes; a failed reindex is
  logged and only heals on the next full `reindex`.

## Links

* [5. Pin digital_inventory edition_id UNIQUE; extend tauri-specta edition-detail IPC](0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md)
* [15. Add get_edition_detail IPC and edition-detail drawer](0015-add-get-edition-detail-ipc-and-edition-detail-drawer.md)
* [46. Enrich edition metadata from plugin enrichers](0046-enrich-edition-metadata-from-plugin-enrichers.md)
