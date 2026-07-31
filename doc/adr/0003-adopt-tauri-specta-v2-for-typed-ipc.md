# 3. Adopt tauri-specta v2 for typed IPC

Date: 2026-07-30

## Status

Accepted

Extended by 5 [5. Pin digital_inventory edition_id UNIQUE; extend tauri-specta edition-detail IPC](0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md)

## Context

`livtet-desktop` is a Tauri 2 + SvelteKit 2 (Svelte 5) application. Before this change, every new Tauri command required three hand-maintained artefacts:

1. A Rust `#[tauri::command]` function in `tauri/src/lib.rs`.
2. A `tauri::generate_handler!` entry that registered the command with the Tauri runtime.
3. A hand-typed TypeScript mirror on the frontend that called `invoke<T>("name", { ... })` and unwrapped the result.

The Tauri command surface was stringly-typed and the return shapes were manually duplicated. Drift between the Rust side and the TypeScript side surfaced only at runtime — usually as a panic in a hard-to-reach code path, since `invoke<SearchHit[]>` only validates the response shape when the promise resolves. The first Tauri command (`greet`) and the planned `search`, `find_edition_by_id`, `find_edition_by_identifier`, and `sync_window_title` commands would each have cost the same maintenance overhead.

The Tauri's `Manager` trait also wasn't in scope inside `app_setup`, which meant `app.path()` failed to compile the moment any module tried to declare itself, blocking the `state.rs` refactor that introduced `AppState`.

## Decision

Adopt [`tauri-specta`](https://github.com/specta-rs/tauri-specta) v2 (`=2.0.0-rc.25`) as the single source of truth for IPC types. Every `#[tauri::command]` is annotated with `#[specta::specta]`, the `tauri_specta::Builder` collects them via the `collect_commands!` macro, and the `specta-typescript` exporter writes `web/lib/bindings.ts` next to the frontend code. The SvelteKit side imports `commands.*` and gets full type info — including the wrapper type for `Result<T, String>` errors, which is `{ status: "ok", data: T } | { status: "error", error: E }` via the runtime's `typedError` helper.

### Wiring

The wiring lives in `tauri/src/lib.rs::run()`. The five currently-registered commands are gathered with:

```rust
let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(
    tauri_specta::collect_commands![
        commands::greet::greet,
        commands::window::sync_window_title,
        commands::search::search,
        commands::edition::find_edition_by_id,
        commands::edition::find_edition_by_identifier,
    ],
);
```

The Builder replaces the previous `tauri::generate_handler![greet]` via `.invoke_handler(specta_builder.invoke_handler())`. The bindings export is gated on `#[cfg(debug_assertions)]` so release builds don't attempt to write into the source tree, and writes to `web/lib/bindings.ts` relative to the binary's CWD (the desktop repo root in `pnpm tauri dev`).

A `generate-bindings` bin (`cargo run --bin generate-bindings`) reproduces the export without booting the Tauri GUI, so CI / quick regeneration doesn't need a runtime. It uses a `pub mod _bindings_export` re-export in `lib.rs` to access the otherwise-private `commands` module.

### State

`AppState` (in `tauri/src/state.rs`) carries the runtime state commands receive via `tauri::State<'_, AppState>`:

- `db: livtet_core::data::SharedState` — the SQLite connection pool.
- `search: Arc<livtet_core::search::SearchIndex>` — the Tantivy index held open for the process lifetime.

`AppDirectories::resolve(app)` (now `async`) constructs both paths via `app.path()`:

- `database_path = <app_local_data_dir>/livtet.sqlite`
- `logs_dir = <app_local_data_dir>/logs/`
- `search_index_path = <app_cache_dir>/search_index`

The `use tauri::Manager;` import is required for `app.path()` to resolve — the method comes from the trait, not from `App` directly.

### Type boundary

The `search` command returns a desktop-local `SearchHitRow` wrapper, not the upstream `livtet_search::SearchHit` directly. The wrapper's only purpose is to re-express `snippet_highlighted: Vec<Range<usize>>` (which `specta-typescript = 0.0.12` refuses to export, since `usize` is a bigint-style type) as `Vec<[u32; 2]>`. The `From<livtet_search::SearchHit>` impl is the only spot where the conversion happens.

The `find_edition_by_identifier` command uses a two-hop lookup: `SELECT * FROM identifiers WHERE value = ?` (URN UNIQUE) → `SELECT editions.* FROM editions JOIN edition_identifiers ON … WHERE identifier_id = ? ORDER BY editions.id ASC LIMIT 1`. The `edition_identifiers` junction is N-to-N (composite PK on `(edition_id, identifier_id)`, not unique on `identifier_id` alone), so the original spec's "deterministic single-row lookup" assumption was wrong; the actual schema reality is captured in the test plan.

### Frontend wiring

- `tauri/src/commands/search.rs` returns `Vec<SearchHitRow>`.
- `tauri/src/commands/edition.rs` returns `Option<EditionRow>` (similar wrapper pattern).
- `web/lib/bindings.ts` is generated; `web/lib/search/types.ts` re-exports `SearchHitRow` as the local `SearchHit` so existing components can keep their stable name.
- `web/routes/+layout.svelte` uses a `MutationObserver` on `document.head > title` to push `document.title` to `commands.syncWindowTitle` on every change. The simpler `\$effect(() => commands.syncWindowTitle(document.title))` only fires once on mount — `document.title` isn't a Svelte reactive signal, so the MutationObserver is the correct pattern.
- `web/routes/search/+page.svelte` debounces the input by 150 ms, then calls `commands.search(query, limit)` from a different `\$effect`. The result is matched against the `Result<T, E>` wrapper to handle `data` vs `error`.

### Tests

The `commands::search` module has a compile-only sentinel test that fires if the wrapper signature ever drifts from what `SearchIndex::search` expects. The `commands::edition` module has six tests:

- Four functional tests: `find_edition_by_id_returns_seeded_row`, `find_edition_by_id_returns_none_for_unknown_id`, `find_edition_by_identifier_returns_linked_edition`, `find_edition_by_identifier_returns_none_for_unknown_urn`.
- Two schema-invariant guards: `duplicate_junction_pair_is_rejected` (composite PK on `(edition_id, identifier_id)`) and `identifiers_value_is_unique` (URN UNIQUE on `identifiers.value`).

`time` and `tokio` are added as dev-dependencies for the test module.

## Consequences

### What becomes easier

- **Every new Tauri command is a 3-line commit.** Declare the function with `#[tauri::command] #[specta::specta]`, add it to `commands/mod.rs`'s `collect_commands!`, and the TS side immediately has `commands.<name>(args)` with full type narrowing. No hand-written wrappers, no schema drift, `svelte-check` fails the build if the call site drifts.
- **The IPC boundary becomes a security review surface.** The command surface is enumerable: every Rust function in `collect_commands!` is auditable in one place, and the generated bindings make the JS side enumerable via `Object.keys(commands)`.
- **The frontend never reads `webview` data without a typed contract.** The `+layout.svelte` `\$effect` mirrors `document.title` into the OS window chrome via `commands.syncWindowTitle`. The `search/+page.svelte` debounced effect calls `commands.search`. Both effects read `commands.*` from typed bindings. Any future route that needs Rust data — fetching a single edition, listing annotations, running a sync — gets the same shape: import, call, narrow the result.
- **Tests stay close to the production code.** The `commands::search` and `commands::edition` modules ship tests next to the production code (the test invariants pin schema behavior that the production code depends on).
- **The bindings regenerate from any workflow.** Three paths all produce the same `web/lib/bindings.ts`: `cargo run -p livtet-desktop` (the runtime export in `app_setup`), `cargo run --bin generate-bindings` (CI / scripting), and the implicit `run()` path. The `web/lib/bindings.ts` entry in `.gitignore` ensures the file is regenerated on every dev build and never accidentally committed.

### What becomes harder

- **`specta-typescript = 0.0.12` has no global bigint toggle.** It refuses `usize`/`isize`/`i64`/`u64`/`i128`/`u128`/`f128` outright. The `search` command's `limit` parameter was `usize`; it's now `u32` because the exporter can't ship a `usize` as `number`. The `SearchHitRow` wrapper exists because `Range<usize>` from `livtet_search::SearchHit` is also refused. Adding any new field that crosses the IPC boundary will likely need the same adapter treatment.
- **The export path is relative to the binary's CWD.** `pnpm tauri dev` runs from the desktop repo root, so `web/lib/bindings.ts` resolves correctly. Anyone running `cargo run` from `tauri/` directly will see the export fail (the path doesn't exist relative to that CWD); that's intentional, not a bug. The `generate-bindings` bin has the same dependency.
- **`mod commands` is private but the bin needs access.** The `pub mod _bindings_export` re-export module in `lib.rs` exists so the bin can call `livtet_desktop_lib::_bindings_export::greet::greet` etc. Don't expose `_bindings_export` to anything but the bin.
- **The existing `greet` command is unused by the UI.** It's still wired through the builder because the migration is a "proof the integration works" example. If it stays unused, a later refactor can drop it — but the command surface is small and the migrator is also a future template, so leaving it is fine.
- **Two output formats in play after the search hit wrapping.** The Rust side keeps `Range<usize>` internally; the IPC side ships `[u32; 2]`. Anyone debugging a serialization mismatch has to remember which boundary they're on.
- **`document.title` is not a Svelte reactive signal.** The first version of `+layout.svelte` had a `\$effect` that read `document.title` and pushed it to the OS chrome. It ran once on mount and never re-fired. The fix is a `MutationObserver` on the `<title>` element (see `+layout.svelte`). Any future "do X on Y DOM change" pattern needs a similar observer; Svelte 5 reactivity tracks runes, not raw DOM mutations.

### What future implementation work can build on this

- **Free use of `#[tracing::instrument]` on every new Tauri command.** The `init_tracing` filter and `AppState` seam make middleware like cross-command tracing, span propagation, or per-command log levels a one-line change.
- **Typed event payloads.** `tauri-specta` derives `Event` for typed payloads. The `mount_events(app)` call in `run()` currently mounts nothing; the moment we need to emit something (e.g. `ReadingProgressUpdate`), the type is auto-generated.
- **A `reindex` command.** Wraps `livtet_search::SearchIndex::migrate_to` so users can recover from a corrupt index without restarting the app. Today `app_setup` is fail-fast; a command-mode recovery is friendlier.
- **Drop the `SearchHitRow` indirection once upstream shapes stabilize.** If `livtet-search` moves `Range<usize>` to `Range<u32>` (or `specta-typescript` ships a `bigint` toggle), the adapter can be removed and the upstream type used directly.
