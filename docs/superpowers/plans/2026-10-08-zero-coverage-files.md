# Zero-Coverage Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cover the logic inside the 26 files currently reporting 0%, and exclude the process wiring that cannot be meaningfully tested, so the coverage report reflects risk rather than noise.

**Architecture:** Most of these files are not uniformly untestable — they hide pure functions inside awkward shells. Tasks 1–4 and 6–7 test those pure cores where they already sit. Task 3 extracts three testable helpers out of an app-bootstrap file so the bootstrap can be excluded without losing them. Task 5 excludes binary entrypoints and the staging script, and updates the coverage gate in lockstep. Tasks 8–9 add browser tests for the two largest components.

**Tech Stack:** Rust (`cargo-llvm-cov`, `#[cfg(test)]` modules, `livtet_core::data::TestDb`), TypeScript/Svelte 5 (vitest `unit` and `browser` projects, `vitest-browser-svelte`).

**Spec:** `doc/adr/0038-self-hosted-code-coverage-with-a-fail-closed-patch-gate.md`

## Global Constraints

- All 10 crates carry `[lints] workspace = true`, and the workspace denies `clippy::unwrap_used` and `clippy::expect_used`. Every new `mod tests` must open with `#![allow(clippy::unwrap_used, clippy::expect_used)]`, matching `crates/livtet-desktop/src/commands/catalog.rs:358`.
- The patch gate is `--fail-under=85` and there is **no `coverage-exempt` override**. Every task must leave `mise run coverage:gate` passing.
- Running the gate locally requires `COVERAGE_COMPARE_BRANCH=github/main`; this repository's only remote is named `github`, not `origin`.
- Verify with `mise test` and `mise lint` per AGENTS.md. Both must be clean before any commit.
- Rust unit tests live in `#[cfg(test)] mod tests` inside the file under test. Frontend unit tests are `web/lib/**/*.test.ts`; component tests are `web/tests/browser/**/*.browser.test.ts` driving a fixture component in `web/tests/browser/fixtures/`.
- These are characterization tests over existing code. Where this plan names a behavior to pin, read the current implementation and assert what it actually does; if what it does looks wrong, record it rather than silently correcting it.

## Review Focus

Five conditions this work will touch that no task's happy path exercises. Each has a test assigned to the task owning the code.

1. **`compute_merge` ignores its `mode` parameter** (`_mode: &str`). Callers passing different modes get identical results. Task 1 pins this explicitly so the dead parameter is visible rather than assumed functional.
2. **`Paths::new` sets `database_path` to the data *directory*** while every sibling field joins a filename. A caller treating it as a file path gets a directory. Task 3 pins current behavior.
3. **`opdsErrorMessage` reads `Object.values(error)[0]`**, so it depends on key insertion order. An error object whose first value is not the message returns the wrong string. Task 6 covers it.
4. **`lua_value_to_sql` on unsupported Lua types** (function, userdata, thread). Task 2 asserts the error path rather than only the five convertible types.
5. **`resolve_plugin_host` with `LIVTET_PLUGIN_HOST` set to a non-existent path.** It returns the path unvalidated. Task 3 pins whether that is intended.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/livtet-desktop/src/commands/enrich.rs` | gains `mod tests` for `compute_merge`, `format_date` |
| `crates/livtet-plugin-host/src/capabilities.rs` | gains `mod tests` for the four conversion fns |
| `crates/livtet-desktop/src/paths.rs` | **new** — `Paths`, moved out of `lib.rs` |
| `crates/livtet-desktop/src/plugin_host.rs` | **new** — `resolve_plugin_host`, moved out of `lib.rs` |
| `crates/livtet-desktop/src/http.rs` | **new** — `reader_status`, moved out of `lib.rs` |
| `crates/livtet-desktop/src/lib.rs` | reduced to bootstrap only; excluded from coverage |
| `crates/livtet-desktop/src/commands/sync.rs` | gains `mod tests` for `parse`, `ok` |
| `mise.toml` | `--ignore-filename-regex` gains entrypoints and `lib.rs` |
| `vitest.config.ts` | coverage `exclude` gains `scripts/stage-pdfjs.mjs` |
| `scripts/normalize-lcov.mjs` | `NOT_MEASURED` gains the newly excluded paths |
| `web/lib/sync.test.ts`, `web/lib/opds.test.ts` | **new** — error mapper tests |
| `web/lib/hotkeys/bindings.svelte.test.ts` | **new** — binding override tests |
| `web/tests/browser/fixtures/SyncPanelFixture.svelte` + `sync-panel.browser.test.ts` | **new** |
| `web/tests/browser/fixtures/LibraryPageFixture.svelte` + `library-page.browser.test.ts` | **new** |

---

### Task 1: Enrichment merge logic

The single largest win: roughly 110 of this file's 162 lines are two pure functions.

**Files:**
- Modify: `crates/livtet-desktop/src/commands/enrich.rs` (append `mod tests`)

**Interfaces:**
- Consumes: `EditionFields`, `EnrichmentPreview` (same file, lines 12 and 25); `Enrichment` and `ImporterPublicationDate` from the importer types.
- Produces: nothing. Internal tests only.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn format_date_renders_year_month_day() { /* (2024, Some(3), Some(7)) -> "2024-03-07" */ }

    #[test]
    fn format_date_omits_absent_day() { /* (2024, Some(3), None) -> "2024-03" */ }

    #[test]
    fn format_date_renders_year_alone() { /* (2024, None, None) -> "2024" */ }

    #[test]
    fn merge_adds_a_field_absent_from_current() { /* current.title_sort None + enrichment Some -> in `added` */ }

    #[test]
    fn merge_reports_a_conflict_when_both_sides_differ() { /* both Some and unequal -> in `conflicts` */ }

    #[test]
    fn merge_unions_list_valued_fields() { /* assert the union, deduped, in `unioned` */ }

    #[test]
    fn merge_collects_found_identifiers() { /* enrichment.found ids -> preview `found` */ }

    // Review Focus 1: the mode parameter is ignored.
    #[test]
    fn merge_ignores_its_mode_argument() {
        // Same inputs under two different modes must produce equal previews.
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `mise exec -- cargo test -p livtet-desktop commands::enrich`
Expected: FAIL — the test module does not exist yet, so compilation or assertions fail.

- [ ] **Step 3: Fill in the test bodies against the real implementation**

Read `compute_merge` (lines 50–154) and construct `EditionFields` / `Enrichment` fixtures. Assert what the function actually does. Add no production code in this task.

- [ ] **Step 4: Run to verify they pass**

Run: `mise exec -- cargo test -p livtet-desktop commands::enrich`
Expected: PASS, 8 tests.

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add crates/livtet-desktop/src/commands/enrich.rs
git commit -m "test(enrich): cover the merge and date-formatting logic"
```

---

### Task 2: Lua/SQL conversion in the plugin host

**Files:**
- Modify: `crates/livtet-plugin-host/src/capabilities.rs` (append `mod tests`)

**Interfaces:**
- Consumes: `canonicalize_all(Vec<PathBuf>) -> Vec<PathBuf>` (line 108); `lua_params_to_sql(Option<&mlua::Table>) -> Result<Vec<rusqlite::types::Value>, String>` (222); `lua_value_to_sql(&mlua::Value) -> Result<rusqlite::types::Value, String>` (234); `sql_value_to_lua` (248).
- Produces: nothing.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn lua_nil_becomes_sql_null() {}

    #[test]
    fn lua_boolean_integer_number_and_string_convert() {}

    // Review Focus 4: the error path, not just the convertible types.
    #[test]
    fn unsupported_lua_value_is_an_error() {
        // A function value must yield Err, not a silent NULL.
    }

    #[test]
    fn absent_params_convert_to_an_empty_vec() {}

    #[test]
    fn param_table_converts_in_index_order() {}

    #[test]
    fn sql_values_round_trip_back_to_lua() {}

    #[test]
    fn canonicalize_all_resolves_real_paths() {
        // Use a tempdir. Pin what happens to a path that does not exist.
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `mise exec -- cargo test -p livtet-plugin-host`
Expected: FAIL.

- [ ] **Step 3: Fill in bodies**

Construct an `mlua::Lua` per test for the value conversions. Read `canonicalize_all` to determine whether unresolvable paths are dropped or passed through, and assert that.

- [ ] **Step 4: Run to verify they pass**

Run: `mise exec -- cargo test -p livtet-plugin-host`
Expected: PASS, 7 tests. This takes the crate from 0%.

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add crates/livtet-plugin-host/src/capabilities.rs
git commit -m "test(plugin-host): cover Lua/SQL value conversion"
```

---

### Task 3: Extract testable helpers from the app bootstrap

`lib.rs` is 230 lines, of which `run()`, `app_setup()`, `init_tracing()`, `setup_database()` and `setup_plugin_host()` are application wiring, and three helpers are pure. Extract the three, then exclude what remains.

**Files:**
- Create: `crates/livtet-desktop/src/paths.rs`, `crates/livtet-desktop/src/plugin_host.rs`, `crates/livtet-desktop/src/http.rs`
- Modify: `crates/livtet-desktop/src/lib.rs` (remove the three, add `mod` declarations, re-export `Paths`)
- Modify: `mise.toml` (add `lib.rs` to `--ignore-filename-regex`)

**Interfaces:**
- Produces: `paths::Paths` with `Paths::new(app_dir: &Utf8PathBuf) -> Self` and its six public fields; `plugin_host::resolve_plugin_host(app_dir: &Utf8PathBuf) -> Utf8PathBuf`; `http::reader_status(status: tauri::http::StatusCode) -> tauri::http::Response<Vec<u8>>`. `lib.rs` keeps re-exporting `Paths` so existing callers are unaffected.

- [ ] **Step 1: Move the three items verbatim into the new modules**

Move `Paths` and its `impl` (lines 125–152) to `paths.rs`, `resolve_plugin_host` (181) to `plugin_host.rs`, `reader_status` (366) to `http.rs`. Add `mod paths; mod plugin_host; mod http;` and `pub use paths::Paths;` to `lib.rs`. Change no behavior.

- [ ] **Step 2: Verify the move compiles with nothing else changed**

Run: `mise exec -- cargo check -p livtet-desktop --all-targets`
Expected: PASS.

- [ ] **Step 3: Write the failing tests**

```rust
// paths.rs
#[test]
fn derives_every_path_from_the_app_dir() {}

// Review Focus 2: database_path is the data directory, not a file.
#[test]
fn database_path_is_the_data_directory_itself() {}

// plugin_host.rs
#[test]
fn env_override_takes_precedence() { /* set LIVTET_PLUGIN_HOST */ }

#[test]
fn falls_back_to_a_path_under_the_app_dir() {}

// Review Focus 5: an override pointing nowhere.
#[test]
fn env_override_is_returned_without_existence_check() {}

// http.rs
#[test]
fn reader_status_carries_the_given_status_code() {}
```

Each new module needs its own `#[cfg(test)] mod tests` with the `#![allow(...)]` header. The env-var tests must not run in parallel with each other — keep them in one `#[test]` or serialize them, since `std::env::set_var` is process-global.

- [ ] **Step 4: Run to verify they fail, then pass**

Run: `mise exec -- cargo test -p livtet-desktop paths plugin_host http`
Expected: FAIL first, then PASS after bodies are filled in against the moved code.

- [ ] **Step 5: Exclude the remaining bootstrap**

In `mise.toml`, anchor the new pattern to this one file:
`(test_support|generate-bindings)\.rs$|^.*livtet-desktop/src/lib\.rs$`.

It must **not** be a bare `lib\.rs$`. **Eight** crates have a measured
`src/lib.rs` in the current report — audio, desktop, epub, importer,
importer-types, mobi, pdf and reader — and `livtet-importer-types` reports
97.9% almost entirely from its `lib.rs`. A bare pattern would silently delete
coverage across all eight, raising the headline number while measuring less.

- [ ] **Step 6: Verify and commit**

```bash
mise run lint && mise run test && COVERAGE_COMPARE_BRANCH=github/main mise run coverage:gate
git add crates/livtet-desktop/src mise.toml
git commit -m "refactor(desktop): extract testable helpers from the bootstrap"
```

---

### Task 4: Sync command helpers

**Files:**
- Modify: `crates/livtet-desktop/src/commands/sync.rs` (append `mod tests`)

**Interfaces:**
- Consumes: `parse<T: serde::de::DeserializeOwned>(serde_json::Value) -> Result<T, SyncError>` (line 17); `ok(&serde_json::Value) -> bool` (23).
- Produces: nothing.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parse_deserializes_a_matching_shape() {}

#[test]
fn parse_maps_a_mismatch_to_a_sync_error() {}

#[test]
fn ok_is_true_for_a_success_envelope() {}

#[test]
fn ok_is_false_for_anything_else() {}
```

- [ ] **Step 2–4: Fail, implement bodies, pass**

Run: `mise exec -- cargo test -p livtet-desktop commands::sync`

The 14 `#[tauri::command]` functions in this file each need a live `AppState` with a running daemon and are **out of scope** — they stay uncovered, and the file will remain well below 85% at file level. That is expected; the gate scores changed lines.

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add crates/livtet-desktop/src/commands/sync.rs
git commit -m "test(sync): cover the response parsing helpers"
```

---

### Task 5: Exclude entrypoints and the staging script

**Files:**
- Modify: `mise.toml` (regex gains `main\.rs$`)
- Modify: `vitest.config.ts` (coverage `exclude` gains `scripts/stage-pdfjs.mjs`)
- Modify: `scripts/normalize-lcov.mjs` (`NOT_MEASURED` gains both)
- Modify: `scripts/normalize-lcov.test.ts`

**Interfaces:**
- Consumes: `selectMeasurable(changedFiles, options)` and the `NOT_MEASURED` list in `scripts/normalize-lcov.mjs`.

**Why the gate script must change in lockstep:** `MEASURED` matches `crates/*/src/*.rs` and `scripts/**/*.mjs`. Once these files are excluded from the reports but still match `MEASURED`, changing one makes preflight assertion 5 fail — a changed measurable file absent from every report. The exclusion is only complete when both sides agree.

- [ ] **Step 1: Write the failing tests**

```ts
test.each([
  ['crates/livtet-plugin-host/src/main.rs', 'binary entrypoint'],
  ['crates/livtet-sync-daemon/src/main.rs', 'binary entrypoint'],
  ['crates/livtet-desktop/src/main.rs', 'binary entrypoint'],
  ['crates/livtet-desktop/src/lib.rs', 'app bootstrap, excluded in Task 3'],
  ['scripts/stage-pdfjs.mjs', 'build staging script'],
])('treats %s as unmeasurable (%s)', (path) => {
  expect(selectMeasurable([path])).toEqual([])
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `mise exec -- pnpm vitest run --project unit scripts/normalize-lcov.test.ts`
Expected: FAIL — these paths currently select as measurable.

- [ ] **Step 3: Add the patterns**

Add to `NOT_MEASURED` in `scripts/normalize-lcov.mjs`, each with its reason as an inline comment: `/(^|\/)main\.rs$/`, `/^crates\/livtet-desktop\/src\/lib\.rs$/`, `/^scripts\/stage-pdfjs\.mjs$/`. Extend the `mise.toml` regex and the `vitest.config.ts` exclude to match.

- [ ] **Step 4: Verify the two sides agree**

Run: `mise run coverage && COVERAGE_COMPARE_BRANCH=github/main mise run coverage:gate`
Expected: preflight passes; `grep -c 'main\.rs' target/coverage/rust.lcov` returns 0; `grep -c 'stage-pdfjs' coverage/lcov.info` returns 0.

- [ ] **Step 5: Commit**

```bash
mise run lint && mise run test
git add mise.toml vitest.config.ts scripts/
git commit -m "build(coverage): exclude binary entrypoints and the staging script"
```

---

### Task 6: Frontend error mappers

**Files:**
- Create: `web/lib/sync.test.ts`, `web/lib/opds.test.ts`

**Interfaces:**
- Consumes: `syncErrorMessage(error: unknown): string` from `$lib/sync`; `opdsErrorMessage(error: unknown): string` from `$lib/opds`.

- [ ] **Step 1: Write the failing tests**

```ts
// sync.test.ts
test('reports the message from an Unavailable error', () => {})
test('reports the message from an Rpc error', () => {})
test('reports the message from a Protocol error', () => {})
test('falls back to an Error instance message', () => {})
test('stringifies anything else', () => {})

// opds.test.ts
test('returns a string error unchanged', () => {})
test('reads a string value out of an error object', () => {})
test('reads a nested message property', () => {})
test('falls back to "OPDS request failed"', () => {})

// Review Focus 3: the result depends on key order.
test('returns the first value, which may not be the message', () => {
  // { code: 'E_NET', message: 'timed out' } returns 'E_NET'.
})
```

- [ ] **Step 2–4: Fail, fill in, pass**

Run: `mise exec -- pnpm vitest run --project unit web/lib/sync.test.ts web/lib/opds.test.ts`

Import only the mapper from each module. The sibling functions call `invoke`, so importing them would pull in the Tauri API; the mappers are pure and need no mocking.

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add web/lib/sync.test.ts web/lib/opds.test.ts
git commit -m "test(frontend): cover the sync and OPDS error mappers"
```

---

### Task 7: Hotkey binding overrides

**Files:**
- Create: `web/lib/hotkeys/bindings.svelte.test.ts`

**Interfaces:**
- Consumes: `bindingFor(id: CommandId): Hotkey`, `hasOverride(id: CommandId): boolean`, `setBinding(id: CommandId, hotkey: Hotkey)`, `resetBindings()` from `$lib/hotkeys/bindings.svelte`.

- [ ] **Step 1: Write the failing tests**

```ts
test('returns the command default when no override is set', () => {})
test('returns the override once one is set', () => {})
test('hasOverride reflects whether a binding was changed', () => {})
test('resetBindings clears every override', () => {})
```

- [ ] **Step 2: Run to verify they fail**

Run: `mise exec -- pnpm vitest run --project unit web/lib/hotkeys/bindings.svelte.test.ts`

- [ ] **Step 3: Handle the persistence side effect**

`setBinding` and `resetBindings` call `void persist()`, which dynamically imports `@tauri-apps/plugin-store`. Under the node environment that import rejects, and because the call is `void`-ed the rejection is unhandled and can fail the run. Mock the module with `vi.mock('@tauri-apps/plugin-store', ...)` returning a stub `load()` whose store resolves. Do not change production code to accommodate the test.

- [ ] **Step 4: Run to verify they pass**

Expected: PASS, 4 tests. The file uses `$state`, so it must keep the `.svelte.ts` extension for rune compilation.

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add web/lib/hotkeys/bindings.svelte.test.ts
git commit -m "test(hotkeys): cover binding overrides and reset"
```

---

### Task 8: SyncPanel component test

Largest component at 0% — 96 coverable lines.

**Files:**
- Create: `web/tests/browser/fixtures/SyncPanelFixture.svelte`
- Create: `web/tests/browser/sync-panel.browser.test.ts`

**Interfaces:**
- Consumes: `web/lib/components/SyncPanel.svelte`. Follow the existing pattern in `web/tests/browser/fixtures/PluginListFixture.svelte` and `plugin-toggle.browser.test.ts`.

- [ ] **Step 1: Read SyncPanel.svelte and the closest existing fixture**

Determine its props and which `$lib/sync` calls it makes on mount. Those calls go through `invoke` and must be stubbed in the fixture or mocked in the test.

- [ ] **Step 2: Write the failing test**

Assert rendered state for at least: the panel's idle/disconnected rendering, and one state transition driven by a prop or a stubbed response. Name each test for the behavior, not the element.

- [ ] **Step 3: Run to verify it fails**

Run: `mise exec -- pnpm vitest run --project browser web/tests/browser/sync-panel.browser.test.ts`

- [ ] **Step 4: Add the fixture until the test passes**

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test
git add web/tests/browser/
git commit -m "test(sync): cover the sync panel's rendered states"
```

---

### Task 9: Library page component test

Largest frontend file at 0% — 146 coverable lines.

**Files:**
- Create: `web/tests/browser/fixtures/LibraryPageFixture.svelte`
- Create: `web/tests/browser/library-page.browser.test.ts`

**Interfaces:**
- Consumes: `web/routes/library/+page.svelte`. `web/tests/browser/library-layout.browser.test.ts` already covers the layout around this route and is the closest reference; the `$app/state` stub is already aliased for the browser project in `vitest.config.ts`.

- [ ] **Step 1: Read the route and the existing library-layout test**

- [ ] **Step 2: Write the failing test**

Cover at least: the empty-library rendering, and a populated list. The route uses TanStack Query, so the fixture must provide a `QueryClient` — check how `library-layout.browser.test.ts` handles this before inventing an approach.

- [ ] **Step 3: Run to verify it fails**

Run: `mise exec -- pnpm vitest run --project browser web/tests/browser/library-page.browser.test.ts`

- [ ] **Step 4: Add the fixture until the test passes**

- [ ] **Step 5: Verify and commit**

```bash
mise run lint && mise run test && COVERAGE_COMPARE_BRANCH=github/main mise run coverage:gate
git add web/tests/browser/
git commit -m "test(library): cover the library page's empty and populated states"
```

---

## Deliberately out of scope

These stay at 0% after this plan, and that is a decision rather than an omission:

- **The 14 `#[tauri::command]` functions in `commands/sync.rs`** and both in `commands/enrich.rs` — each needs a live `AppState`. `commands/catalog.rs` shows this is possible with `TestDb`; it is simply a larger piece of work than this plan covers.
- **`crates/livtet-desktop/src/commands/sync.rs` and `src/sync.rs` beyond their helpers**, and `setup_database` / `setup_plugin_host`.
- **Thirteen smaller components**: `CatalogSettings` (81), `EditionDetailDrawer` (57), `FilterPanel` (47), `AddPluginPanel` (38), `AudioPlayer` (29), `CommandPalette` (28), `TagPicker` (15), `HotkeyRow` (12), `PlayerProgress` (12), `catalog/[catalogId]/+page` (49), `catalog/+page` (12), `+layout` (35), `BookCard` (10) — 425 coverable lines between them. Tasks 8 and 9 establish the pattern; these follow it.

A follow-up plan should take the remaining components in one pass, once Tasks 8 and 9 have shown what a fixture for a query-driven component looks like in this codebase.
