# 46. Enrich edition metadata from plugin enrichers

Date: 2026-10-30

## Status

Accepted. Implemented end to end: preview, the three modes, apply with a
transaction and post-commit cover, the review screen. Bulk enrichment remains
out of scope — see Implementation status.

## Context

The edition-detail drawer shows a catalog record but offers no way to improve
it. Import creates records; nothing until now has modified edition metadata, so
a thin record imported from a filename stays thin.

The substrate already exists. The plugin host runs signed Lua plugins
out-of-process (ADR-0029), plugin management lists/installs/removes them
(ADR-0030), the set of loaded plugins *is* the set of directories under the
plugins root (ADR-0034), and sandbox modules include an app-mediated `http`
capability (ADR-0031). `livtet-importer` defines the contract —
`Enricher::{new, sources, enrich}` plus `BookIdentity` and `Enrichment` — and a
bundled OpenLibrary enricher ships under `plugins/openlibrary`, declaring
`[capabilities.http] domains = ["openlibrary.org", "covers.openlibrary.org"]`.

What is missing is everything between the contract and the drawer: nothing
dispatches `sources`/`enrich`, nothing merges, nothing writes.
`commands/enrich.rs` is a stub that merges hard-coded fixtures, ignores its
`mode` argument, and writes nothing; its `enrich_library` twin returns
`unavailable`.

Alternatives considered:

1. **Apply immediately, no preview.** One click overwrites catalog fields with
   whatever a third-party source returned. Rejected: a metadata overwrite with
   no review step, from a source the user did not choose.
2. **Apply with undo.** Needs the previous values persisted to revert to, which
   is a new history table for a feature whose whole point is a preview.
3. **A user-facing plugin picker.** A choice to make on every run, when the
   identifiers already say which source can answer.
4. **First match wins.** Ignores a second source that knows more (a publisher
   or a description the first one lacks).
5. **Skip covers, or invent `digital_inventory` rows** for file-less editions.
   Inventing rows would corrupt the `has_file` filter and card badges.
6. **Fill-only, or per-field checkboxes.** Fill-only removes any way to take a
   better value; per-field selection is the largest UI and needs a
   field-selection payload in apply.
7. **Re-run the lookup on apply,** or round-trip the fetched metadata through
   the webview. Re-running doubles the network and lets the applied result
   differ from the previewed one; round-tripping makes a database write trust
   client-supplied metadata.

## Decision

### Preview, then confirm

Two commands, registered in `collect_commands!`:

```rust
preview_enrichment(edition_id: String) -> Result<EnrichmentPreview, PluginError>
apply_enrichment(edition_id: String, mode: EnrichMode) -> Result<EnrichmentSummary, PluginError>
```

The preview takes no mode: classification into added/unioned/conflicts does not
depend on it, so one preview serves every mode and flipping the toggle refetches
nothing.

### Three modes: fill, union, overwrite

`EnrichMode` is a real enum (`specta` emits `"fill" | "union" | "overwrite"`),
not a free `String`, so a toggle cannot send a typo.

| | absent scalar | differing scalar | list already populated | `found` identifiers |
| --- | --- | --- | --- | --- |
| `fill` | added | conflict, current kept | untouched | appended |
| `union` | added | conflict, current kept | missing values appended | appended |
| `overwrite` | added | conflict, **incoming wins** | missing values appended | appended |

Lists are never shrunk in any mode. The source does not know which local
subjects or contributors to remove, and dropping them would destroy curation
that no preview can restore. `overwrite` therefore governs scalars and the
cover, not membership.

`Enrichment.title` becomes writable (the stub ignored it) and `title_sort` gains
a conflict branch; today a differing `title_sort` is silently dropped.

### One merge function

`merge(current: &EditionFields, incoming: &Enrichment, mode: EnrichMode) -> Merge { added, unioned, conflicts, applied }`.
Preview renders it; apply writes `applied`. Because both paths call the same
function, the preview cannot drift from what is written. `combine` folds
several plugins in order: first non-`None` scalar wins, lists concatenate then
dedupe preserving order, `found` dedupes by `(kind, value)`. Both are pure.

### Automatic matching on identifiers

Dispatch `sources` across the installed plugins, keep those whose declared kinds
intersect the edition's identifiers, then `enrich` each in plugin order. The
identity is read server-side from `edition_identifiers`; the command does not
trust the client for it. A plugin that cannot answer `sources` is an importer,
not an enricher, and is skipped rather than treated as a failure.

### The `http` trust boundary

The allowlist is **not** the grant alone. The callback travels from the host
process to the app, so `CallbackCall.grant` is chosen by the child; stanchion
says so at `crates/stanchion-remote/src/client.rs:268-277` — "Re-checking
`call.grant` proves nothing, because the child chose it… treat `call.grant` as
an untrusted hint."

`answer_http(call, plugins_dir)` therefore takes the intersection of two inputs:
the app's own read of `<plugins_dir>/<plugin>/plugin.toml` (the trust anchor) and
the forwarded grant (a narrowing hint). A plugin name that is not a single path
segment reads nothing; an unreadable or unparsable manifest grants nothing;
non-string and blank entries are dropped. An empty list on either side refuses
every host, with a distinct message per case. The enricher host client binds
`on_callback` with that policy and restricts `allow_capabilities(["http"])`, so
no other capability reaches the handler.

### Cache the fetched enrichment

`AppState` gains `enrichment_cache: ArcMut<HashMap<String, CachedEnrichment>>`
holding the combined `Enrichment`, its contributing plugin names and a
timestamp. A new preview replaces the entry; apply removes it; entries older
than ten minutes are dropped on access and a 32-entry cap evicts oldest. Apply
with no entry returns `invalid("preview expired; run Enrich again")` rather than
silently refetching.

### Persistence

One transaction, in the order `import.rs` uses, then the cover after commit to
`covers/{inventory_id}/cover.{ext}` per ADR-0008 — cleared if the write fails,
the same post-commit fallback import already has. The released edition is
reindexed so title, publisher, language and subjects stay searchable.

The catalog writers this needs (`upsert_author`, `upsert_identifier`,
`upsert_publisher`, `upsert_subject`, `resolve_language`, `contributor_bindings`,
`unique_preserving_order`) exist but are private to `import.rs`, which is past
1400 lines and growing. They move to `commands/catalog_write.rs`; both callers
use it, and the existing import tests guard the extraction.

### Frontend

`web/lib/library/enrich.ts` holds the invokers plus the pure helpers
(`previewIsEmpty`, `enrichDisabledReason`, `summaryMessage`), reusing
`pluginErrorMessage` from `web/lib/plugins.ts` rather than adding a second
unwrapper. Enrich sits in the drawer header, disabled with an explanatory
`title` when the edition has no identifiers. A preview switches the drawer body
to a review screen (Added / Unioned / Conflicts / Found / Cover, a back arrow,
the mode control, Apply / Cancel); conflicts show `current` and `incoming`
separately and dim under the modes that keep the current value. After apply:
invalidate `catalogKeys.editionDetail` always, `catalogKeys.all` when a cover was
stored, `searchKeys.all` when any field changed.

## Consequences

- The first surface in the app that writes edition metadata. Every later writer
  (merge duplicates, manual edits) inherits the transaction shape and the
  `catalog_write` helpers rather than inventing its own.
- One merge function makes preview/apply drift a compile-time impossibility, and
  makes mode semantics testable without a host, a database or a network.
- Enrichment stays optional and plugin-specific: no provider is compiled in, and
  a source is added by dropping a directory in the plugins root (ADR-0034).
- The trust boundary now depends on the app reading the manifest itself. A
  future per-capability grant store (deferred in ADR-0030) would replace that
  read, not the intersection rule.
- `overwrite` can replace prose a user curated by hand. Accepted: it is never the
  default, and the preview lists every conflict before it lands.
- Bulk enrichment of a whole filter is **out of scope**; `enrich_library` stays
  `unavailable`, retyped to `EnrichMode` so its signature matches.
- Per-field selection, a plugin picker, enriching works rather than editions, and
  covers for editions without a `digital_inventory` row are all out of scope.

## Implementation status

| Piece | State |
| --- | --- |
| `http` trust model (manifest read ∩ grant, `allow_capabilities`) | landed, tested |
| Enricher host client (`sources` dispatch, match, `enrich`, decode) | landed, tested against a fake plugin and a loopback server |
| `catalog_write` extraction | landed (`commands/catalog_write.rs`, callers: import + enrichment) |
| `EditionFields` / `BookIdentity` read from SQLite | landed (`commands/enrich_store.rs::read_snapshot`) |
| `merge` with fill/union/overwrite, `combine` | landed (`commands/enrich_merge.rs`); classification is mode-independent, only the resolved write set follows the mode |
| `EnrichMode` + the two-command IPC contract, bindings regeneration | landed (`preview_enrichment`, `apply_enrichment`; `enrich_library` retyped, still unavailable) |
| Cache, transaction, cover, reindex | landed (10-minute TTL, 32-entry cap; one transaction, cover after commit, `add_edition` reindex) |
| `enrich.ts` + drawer review screen | landed (`web/lib/library/enrich.ts`, review screen with the mode control) |

## Testing

Unit tests for `merge` (each mode, empty current, list dedupe, conflict
detection, cover storable or not), `combine` (first non-`None` wins, order
preserved, `found` dedupe) and the grant policy (manifest only, grant only,
disjoint, unsafe name). A host-backed integration test installs a temporary fake
enricher that returns a fixed `Enrichment`, asserting preview classification with
no network. A persistence test applies against a temporary database in the style
of `commands::import::tests`. Vitest covers the `enrich.ts` helpers, and the
review screen is covered by the browser test harness under `web/tests/browser/`
(the same fixture pattern the drawer and reader already use) — so the button and
review screen are verified, not hand-checked.

## Follow-ups

- **ADR-0031 amendment** — record that the `http` allowlist is the intersection
  of the app's own manifest read (the trust anchor) and the host-forwarded grant
  (an untrusted hint), and that an empty list on either side refuses every host.
- When this lands, the second writer of edition metadata (a merge-duplicates or
  manual-edit command) should extend `catalog_write` rather than `import.rs`.
