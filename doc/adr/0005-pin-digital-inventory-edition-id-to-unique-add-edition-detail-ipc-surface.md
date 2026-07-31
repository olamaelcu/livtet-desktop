# 5. Pin digital_inventory edition_id UNIQUE; extend tauri-specta edition-detail IPC

Date: 2026-07-30

## Status

Accepted

## Context

`livtet-desktop` (Tauri 2 + SvelteKit 2 / Svelte 5) ships with two
catalog-related inventory tables in `core/livet-data`:

- `digital_inventory` (m0004, extended by m0007 for cover metadata):
  catalog-side metadata about a digital copy of an edition.
  Carries `file_path`, `file_hash`, `file_size_bytes`, plus the
  cover-metadata columns (`cover_path`, `blurhash`, `dominant_color`).
  The seed in `add_digital_inventory` writes exactly one row per
  edition via `ws.primary_edition_id`.
- `edition_files` (m0010): plugin-scanned file tracking with
  `file_format`, `file_mode` (`link|symlink|copy`), `source_plugin`,
  `source_id`. Designed 1:N — an edition can exist as multiple
  physical files (EPUB + PDF).

Three properties of the pre-change `digital_inventory` schema blocked
the next planned desktop feature (a per-edition detail view exposing
"files on disk"):

1. **Schema didn't match data-model intent.** `digital_inventory.edition_id`
   had no UNIQUE constraint, even though the seed and the OPDS
   server's `HashMap<DbId, Model>::collect` (in
   `opds/opds-server/src/repository_live.rs::assemble_works`) treated
   it as 1:1. New writers could silently insert duplicates; the
   `HashMap` collect became a data-shadowing bug.

2. **No index on `edition_id`.** Every lookup was a sequential scan.

3. **No typed IPC surface for per-edition detail.** ADR-0003 established
   the `EditionRow` wrapper pattern, but only `find_edition_by_id` and
   `find_edition_by_identifier` existed. The catalog detail view (Files
   / Authors / Identifiers tabs) needed three more commands.

## Decision

### 1. Schema: pin `digital_inventory.edition_id` UNIQUE

[olamaelcu/livtet#1](https://github.com/olamaelcu/livtet/pull/1) added
migration `m0011_digital_inventory_unique_edition` that creates the
unique index `uq_digital_inventory_edition_id`. Idempotent via
`has_index` guard. The SeaORM entity gained `#[sea_orm(unique)]` on
`edition_id` so the generated query DSL matches. The `db_error.rs`
parser gained a `PrimaryKey::DigitalInventoryEdition` variant routing
`digital_inventory.edition_id` UNIQUE violations to a user-friendly
message; two unit tests pin the parser behaviour.

`edition_files` (m0010, 1:N) was deliberately **not** touched. The
two-table model is now explicit:

| Table | Cardinality | Provenance | Index on edition_id |
|---|---|---|---|
| `digital_inventory` | 1:1 (post-m0011) | catalog metadata | `uq_digital_inventory_edition_id` (UNIQUE) |
| `edition_files` | 1:N | plugin-scanned | `idx_edition_files_edition_id` (non-UNIQUE) |

No pre-flight dedup step. Acceptable assumption: no production
install exists yet.

### 2. OPDS: trust the schema, not a SAFETY comment

[olamaelcu/livtet-opds#1](https://github.com/olamaelcu/livtet-opds/pull/1)
landed after an iteration: the first revision added a SAFETY comment
to the `HashMap::collect` line, the second replaced it with a
`HashMap::try_insert` defensive layer that surfaced duplicates as
`OpdsError::Database`, and the third reverted both in favour of
trusting the schema — a 4-line comment documenting that the bulk
fetch + `into_iter().collect::<HashMap<…>>()` cannot collide because
the schema enforces uniqueness.

The lesson: when the database is the source of truth, surface the
invariant in the database, not in defensive code. The Rust-side
`try_insert` was complexity defending against an impossible state.

### 3. Desktop: extend the EditionRow wrapper pattern

Per ADR-0003, three new Tauri commands using the same wrapper shape
(landed via [olamaelcu/livtet-desktop#2](https://github.com/olamaelcu/livtet-desktop/pull/2)):

- `commands::digital_inventory::find_files_by_edition` →
  `Result<Option<DigitalInventoryRow>, String>`. The m0011 invariant
  means `.one()` is provably-at-most-one; return shape narrows to
  `Option<…>` instead of the original `Vec<…>` sketch.
- `commands::edition_identifiers::find_identifiers_by_edition` →
  `Result<Vec<IdentifierRow>, String>`. Two-hop JOIN through
  `edition_identifiers` to `identifiers`. Schema invariants pinned
  in ADR-0003 (`identifiers.value` UNIQUE) still apply.
- `commands::edition_authors::find_authors_by_edition` →
  `Result<Vec<AuthorWithRole>, String>`. JOIN through `edition_authors`
  to `authors`. Wrapper carries `role` so the UI can label translators
  / editors without an extra lookup.

Each command follows the same module structure as `commands/edition`:
wrapper struct + `From<livtet_core::data::entities::…::Model>` + the
command function + `#[cfg(test)] mod tests` with happy-path and
schema-invariant guards. Bindings regenerate via
`cargo run --bin generate-bindings` (the same workflow documented in
ADR-0003).

### 4. Frontend: catalog detail view

A new `web/lib/catalog/` module introduces two surfaces that share
one `<wa-tab-group>` host (`<EditionDetail>`):

| Surface | Trigger | URL/State | Closes via |
|---|---|---|---|
| Quick peek | click on cover-card | `peekState` (Svelte 5 rune in `.svelte.ts`) | `light-dismiss`, Escape, close button |
| Deep-link route | typed/pasted URL | `/catalog/[editionId]` SvelteKit param | browser back, in-page close → `goto("/search")` |

`<wa-tab-group>` is the first use of this element family in the
codebase. Both surfaces mount the same `<EditionDetail>` component so
any future tab-pattern tweak lands in one place.

## Consequences

### Becomes easier

- Every future "show me the digital inventory for X" query is an
  indexed `.one()`. Duplicate-shadowing in OPDS goes from latent
  bug to impossible.
- Adding new per-edition detail tabs is now a 3-line commit per
  ADR-0003 (`commands::…` + `collect_commands!` entry + auto-generated
  TS binding).
- The pattern of "test next to production code" extends naturally to
  `commands::digital_inventory`, `commands::edition_authors`,
  `commands::edition_identifiers`. Each module pins one or more schema
  invariants in its test suite.
- `find_files_by_edition` returns `Option<…>`, not `Vec<…>`. The
  frontend's `<EditionDetail>` Files tab renders a single row object
  rather than an `{#each}` loop — less branching on the UI side.

### Becomes harder or carries risk

- `specta-typescript = 0.0.12` refuses `usize`/`isize`/`i64`/`u64`.
  `digital_inventory.file_size_bytes` (originally `Option<i64>`) had
  to be re-typed as `Option<f64>` in the IPC wrapper because specta
  can't ship a `usize` as `number`. Values ≤ 2^53 (~9 PB) round-trip
  exactly; downstream consumers should know it's now a JS `number`.
- `<wa-tab-group>` is introduced in two places (peek + route). Any
  future tab-pattern tweak (lazy-mounting, keyboard nav) needs to land
  in both.
- The `PrimaryKey` enum in `core/livtet-data` originally mixed composite
  PKs (junction tables) with single-column UNIQUE indexes (m0011).  As
  of 2026-07-31, `DigitalInventoryEdition` was extracted into a new
  `UniqueIndex` enum so the `strum(prefix = "pk_")`-derived `Display` no
  longer produces misleading names.  `ConstraintViolation` gained a
  matching `UniqueIndex(UniqueIndex)` variant with its own scan pass.
- `desktop/web/lib/bindings.ts` is gitignored and regenerated by
  every `cargo run -p livtet-desktop`. A stale bindings file would
  fail `pnpm check` but not `cargo check` — easy to miss in CI if
  `pnpm check` isn't part of the gate.

### What future work can build on this

- Cross-link `find_authors_by_edition` with `find_authors_by_work` so
  the Overview tab can show "primary authors for this work, plus any
  contributors specific to this edition."
- Cover image serving via Tauri's asset protocol, fed by
  `digital_inventory.cover_path` / `digital_inventory.blurhash`.
  Separate ADR.
- File-format metadata joins: `digital_inventory` has `file_path` but
  not `file_format`. Either add a column (new migration) or join to
  `edition_files` by `file_path` (additive view).
- A `reindex_after_inventory_change` Tauri command — Tantivy's search
  index currently doesn't see `digital_inventory` rows; if search
  ranking ever wants to consider "has file on disk," that's the seam.
- ~~The `PrimaryKey` enum's mixed semantics (composite PK + single-column
  UNIQUE) deserves a future split — `UniqueIndex` enum for the
  single-column case, `PrimaryKey` for the composite case.~~  **Done
  (2026-07-31).**  See `core/livtet-data/src/unique_index.rs`.

## Links

* [Extended by 5](0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md)
* [3. Adopt tauri-specta v2 for typed IPC](0003-adopt-tauri-specta-v2-for-typed-ipc.md)
