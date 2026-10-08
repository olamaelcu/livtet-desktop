# 38. Self-hosted code coverage with a fail-closed patch gate

Date: 2026-10-08

## Status

Accepted

## Context

The repository has no coverage measurement. Tests are plentiful — roughly
332 `#[test]`/`#[tokio::test]` functions across ten crates, 21 vitest unit
files, seven vitest browser-mode component tests, three Playwright e2e specs
— but nothing reports which code they actually exercise, so there is no way
to find untested areas or to notice coverage falling.

Three outcomes were wanted: a map for finding untested areas, a ratchet so
coverage cannot silently fall, and visibility on pull requests. A public
badge and a historical trend line were considered and dropped; the patch
gate delivers the ratchet without needing stored history, and dropping the
badge means no workflow ever needs `contents: write`.

A hosted service (Codecov) would supply all of this from one integration.
It was rejected to avoid a third-party dependency holding the gate, keeping
the enforcement logic reviewable in-repo. That choice has a specific,
well-understood hazard: **a self-hosted gate's failure mode is a green
check.** The gate joins lcov records to a `git diff` on file paths and line
numbers. When the join silently misses, the result is "no changed lines
needed coverage" — reported as success. A gate that fails open is worse
than no gate, because it stops anyone from looking.

That hazard is not hypothetical here. Two pieces of CI glue already fail
silently and permissively:

* `cq.yml` runs `nose query ... || true` and falls back to
  `{"families": []}` on any parse error, so a broken duplication check
  reports "Found **0** copy-paste families" — indistinguishable from a
  clean repository.

* `ci.yml`'s `CI Summary` job gates its steps on `needs.all.result` and
  `needs.any.result`. Neither context exists; both conditions evaluate
  falsy, both steps skip, and the job succeeds unconditionally even when
  `lint` and `test` fail.

Designing against this failure mode is therefore the central constraint of
this ADR, not a detail of it. This decision extends ADR 6, whose rule that
CI only invokes mise tasks is preserved.

## Decision

Measure coverage with two per-toolchain producers emitting lcov, join them
to the diff with `diff-cover`, and refuse to report a result unless a set
of preflight assertions hold.

### Measurement

* **Rust** — `cargo-llvm-cov` 0.9, pinned as `"cargo:cargo-llvm-cov"` in
  `mise.toml` following the `cargo:cargo-machete` precedent. Invoked
  `--workspace --all-targets --lcov`. Doctests are excluded, since
  `cargo-llvm-cov` covers them only on nightly; `mise test` continues to
  run `cargo test --doc` separately as an uncovered check.

  The task creates `target/coverage` itself: `cargo-llvm-cov` does not
  create its output directory and fails report generation if it is absent.
  `test_support.rs` files are dropped via `--ignore-filename-regex`, since
  they are test scaffolding rather than product code — the same reason the
  frontend config excludes `web/tests/**`. No dependency exclusion is
  needed: every record falls inside the repository root, so the
  git-sourced dependencies are already absent, and integration tests under
  `crates/*/tests/` are compiled by `--all-targets` but never reported as
  source.

* **Frontend** — `@vitest/coverage-v8` pinned to **4.1.11**. The current
  latest is 5.0.3, which is incompatible with the repository's vitest
  4.1.10. Vitest's own packages pin each other to an exact patch, not
  merely a matching major, so the whole family — `vitest`,
  `@vitest/coverage-v8`, `@vitest/browser-playwright` — moves together. Coverage is configured once at
  the root of `vitest.config.ts` so a single `vitest run --coverage`
  aggregates the `unit` and `browser` projects. Reporters: `lcovonly`,
  `html`, `text-summary`.

  No additional vitest plugin is required for `.svelte` files.
  `@sveltejs/vite-plugin-svelte`, already a devDependency, compiles the
  components and emits the source maps that `@vitest/coverage-v8` consumes;
  there is no separate Svelte coverage plugin.

  Two configuration constraints are load-bearing. First,
  **`coverage.include` must be set explicitly** to
  `['web/lib/**/*.ts', 'web/**/*.svelte']`: vitest 4 removed
  `coverage.all`, so without `include` only files imported during the run
  appear in the report. Second, v8 coverage in browser mode requires a
  Chromium-based browser, which the `browser` project's single `chromium`
  instance already satisfies.

  `vitest --merge-reports` must not be used with the v8 provider. There is
  an open, untriaged native crash (malloc abort / SIGSEGV) in that
  combination on exactly this repository's versions — vitest 4.1.x with
  vite 8 — suspected to involve Rolldown. The single-invocation design
  above avoids it; istanbul is the workaround if merging ever becomes
  necessary.

* **Not measured** — Playwright e2e, which would require instrumented
  builds and report merging for a flakier number. Excluded from reports:
  `web/lib/bindings.ts` (generated from Rust enums, already force-ignored
  by Biome per ADR 6), `web/tests/**`, `e2e/**`, `.svelte-kit/**`,
  `build/**`, `test_support.rs`, and
  `crates/livtet-desktop/src/bin/generate-bindings.rs` (a build-time dev
  tool, not shipped product).

  Two corrections to an earlier draft of this list. `crates/*/build.rs`
  needs no exclusion: cargo-llvm-cov never reports build scripts, verified
  against a real report. And `scripts/**` is **not** excluded — the gate
  script is deliberately measured, so `coverage.include` covers
  `scripts/**/*.mjs`. The code holding the gate being subject to the gate
  is the point, not an oversight.

`crates/livtet-desktop` is largely Tauri IPC glue that is not unit-testable
and will read low. The number is a map, not a grade.

### Fail-closed preflight

`scripts/normalize-lcov.mjs` rewrites lcov `SF:` records to repo-root-relative
POSIX paths — `git` emits those, while both producers tend toward absolute —
and then hard-errors unless all of the following hold:

1. Both lcov files exist, are non-empty, and parse.
2. Each yields at least a floor count of `SF:` records, catching "the tool
   ran and produced an empty report."
3. Every normalized `SF:` path exists on disk; one that does not means
   normalization has drifted.
4. The merge base against `origin/main` resolved to a real commit, which
   requires `fetch-depth: 0` and so also catches a later "optimization"
   back to depth 1.
5. Every changed, measurable, non-excluded file from `git diff --name-only`
   appears in the combined lcov. **This is the assertion that converts the
   silent-pass bug into a build failure.** It holds only because untested
   files still appear in both reports, which for the frontend is true only
   while `coverage.include` is correctly set — vitest 4 reports nothing it
   did not import. A mistake in `include` therefore disarms the very
   assertion meant to catch silent passes, so the preflight treats an
   empty-looking frontend report as a hard error (assertion 2) rather than
   as a repository with no frontend code.

Assertion 5 needs a tolerance list for files that legitimately cannot be
measured. That list lives in the script as named constants, not as a regex
nobody can read.

The script is `.mjs` so the vitest `unit` project can test it. **Unit tests
for it are mandatory**, not optional: untested CI glue that fails silently
is precisely the risk this ADR exists to contain, and the gate's own logic
must not be the one unmeasured thing in the repository.

### Gate and reporting

`diff-cover` 10.6, pinned as `"pipx:diff-cover"` following the
`pipx:taskwarrior-mcp` precedent, consumes both lcov files directly — it
accepts multiple reports, so there is no merge step to get wrong — with
`--compare-branch=origin/main` for merge-base semantics and
`--fail-under=85`. It exits 1 below the threshold and 0 above it, so the
gate fails the build on its own exit status with no wrapper logic.

CI disables the taskwarrior tools through `MISE_DISABLE_TOOLS`. That list
matches exact tool names, so it does not catch this pin; confirmed by the
taskwarrior entries vanishing from `mise ls` under that variable while
`pipx:diff-cover` remains.

The threshold applies to **all** measured file types, `.svelte` included.
85% is deliberately above current practice: the measured baseline below
puts this repository's existing component tests at 73–90% on the files they
target, and that range is treated as a floor to improve on rather than a
bar to match. The gate will therefore bite early and often, so it ships
with a deliberate escape hatch: a `coverage-exempt` pull request label
skips the gate and writes a prominent line into the step summary naming the
exemption. An override that leaves a trace is what keeps people off the
override that does not — commenting out the gate.

### Measured baseline

Frontend coverage was stood up first, before the gate existed, to replace
projections with numbers. Measured at commit `75c59d0`, 160 tests across 25
files:

| Scope | Files | Line coverage |
|---|---|---|
| `web/lib/**/*.ts` | 24 | 72.5% (303/418) |
| `web/**/*.svelte` | 40 | 28.6% (302/1055) |
| Overall | 64 | 41.1% (605/1473) |

Among components with at least ten coverable lines, those that have browser
tests scored: `NavRail` 100%, `LibraryToolbar` 90.0%, `PluginList` 80.6%,
`reader/pdf/[editionId]/+page` 76.8%, `AddBookDrawer` 73.4%. The remaining
fifteen substantial components are at 0%.

Two findings from this run shape the rest of the work:

* **`coverage.include` works as assertion 5 requires.** 23 of 40 components
  report with zero lines hit, so untested files do appear. The file count
  reconciles exactly — 53 `.svelte` on disk, less 13 excluded test fixtures,
  equals the 40 reported — confirming nothing is silently absent.

* **The two producers disagree on path form.** Frontend lcov `SF:` records
  are already repo-root-relative (`web/lib/notifications.svelte.ts`), while
  `cargo-llvm-cov` emits absolute paths
  (`/home/.../crates/livtet-mobi/src/cover.rs`). Normalization is therefore
  real but one-sided: strip the repository root prefix from the Rust
  records and leave the frontend records untouched. Assertion 3 is what
  catches it if either producer's form ever changes.

### Rust baseline

Measured at commit `b80a614`, workspace-wide, `test_support.rs` excluded:

| Crate | Line coverage |
|---|---|
| `livtet-importer-types` | 97.9% (94/96) |
| `livtet-epub` | 94.0% (1404/1494) |
| `livtet-mobi` | 92.7% (522/563) |
| `livtet-audio` | 90.2% (230/255) |
| `livtet-reader` | 85.4% (309/362) |
| `livtet-pdf` | 82.5% (241/292) |
| `livtet-importer` | 77.1% (290/376) |
| `livtet-desktop` | 65.5% (4727/7222) |
| `livtet-plugin-host` | 0.0% (0/314) |
| `livtet-sync-daemon` | 0.0% (0/30) |
| **Workspace** | **71.0% (7817/11004)** |

The format-parsing crates sit at or above the 85% gate already.
`livtet-desktop` at 65.5% is markedly better than this ADR originally
predicted for Tauri IPC glue, so the expectation that it would "read low"
is withdrawn.

Note that `diff-cover` scores changed lines, not whole files, so these
file-level figures overstate the gate's day-to-day difficulty: a change of
twenty lines to a file sitting at 73% passes at 85% if seventeen of those
twenty lines are covered.

### What the gate would have said

Run against a real historical range — 44 changed source files, 842 diff
lines, spanning the PDF reader work — `diff-cover` reports **76% patch
coverage**, with 202 lines missing. Against the 85% threshold that change
would have been blocked.

The shortfalls are concentrated where the baselines predict: `enrich.rs`
and `capabilities.rs` at 0%, `reader.rs` at 73.9%, the PDF reader route at
74.3%, and several route components at 0%. Two frontend modules
(`reader/pdf.ts`, `reader/read.ts`) came in at 100%.

This is the clearest available evidence about the threshold: work of this
shape and size does not currently clear 85% without additional tests. That
is the intended pressure, recorded here so it is not mistaken for a
malfunction the first time it happens.

Visibility is `diff-cover --markdown-report` appended to
`$GITHUB_STEP_SUMMARY` (the pattern `cq.yml` uses) plus `--html-report`
uploaded as an artifact (the pattern `ci.yml` uses for `e2e-report`). No
badge, no trend file, no `contents: write` in any job.

### Mise tasks

| Task | Does |
|---|---|
| `coverage:rust` | `cargo llvm-cov` → `target/coverage/rust.lcov` |
| `coverage:web` | `vitest run --coverage` → `coverage/lcov.info` |
| `coverage` | depends on both; normalizes paths, writes HTML + text summary |
| `coverage:gate` | depends on `coverage`; runs the preflight, then `diff-cover` |

### CI

A new `coverage` job in `ci.yml` running parallel to `lint` and `test`,
with `fetch-depth: 0`, `rustup component add llvm-tools-preview`, its own
`Swatinem/rust-cache` key since instrumented artifacts differ from the test
job's, the existing apt prerequisites block, Playwright chromium for the
browser project, then `mise run coverage:gate` and an `if: always()` HTML
artifact upload. `coverage` is added to the `CI Summary` job's `needs`.

### Incidental fixes

Three pre-existing defects are corrected because this work touches the same
files and would otherwise inherit them:

* `Swatinem/rust-cache` is configured `workspaces: tauri -> target` in both
  the `lint` and `test` jobs, but **there is no `tauri/` directory** — the
  config lives at `crates/livtet-desktop/tauri.conf.json`. The Rust cache
  is therefore ineffective and CI rebuilds the workspace cold. Fixing this
  offsets the cost of adding a second Rust build.

* `CI Summary`'s `needs.all` / `needs.any` conditions are replaced with
  `contains(needs.*.result, 'failure')`. Shipping a new gate while the
  summary job passes unconditionally would be self-defeating.

* `package.json`'s `tauri:dev` and `tauri:build` pass the same stale
  `--config tauri/tauri.conf.json` path, masked because `mise run dev`
  calls plain `pnpm tauri dev`.

The untracked `vitest.config.ts.bak` was deleted so it cannot be picked up
by coverage include globs.

## Consequences

### Becomes easier

* Untested areas become visible: `mise run coverage` produces a browsable
  HTML report locally with the same command CI runs.
* New code carries tests or the build says so, without retro-testing the
  existing surface.
* The gate's logic is readable, reviewable, and unit-tested in-repo, with
  no external service able to block or mis-score a merge.
* The Rust build cache starts working, which it has not been.

### Becomes harder

* CI wall-clock roughly doubles for the Rust half, since coverage needs a
  separately-cached instrumented build. Free on a public repository, but
  noticeable.
* Assertion 5's tolerance list is maintenance: an unmeasurable file not yet
  listed fails the build. This is deliberate — the alternative is silence.
* An 85% patch gate including `.svelte` is above what four of this
  repository's five currently-tested components achieve at file level, so
  it will block route work until component tests get more thorough than the
  present norm. This is intended, not incidental. The `coverage-exempt`
  label is the sanctioned relief valve and should be expected to see real
  use in the early weeks.

### Risks to mitigate

* **`.svelte` denominators, not `.svelte` attribution.** An earlier draft
  of this decision recorded line attribution through Svelte's compiled
  output as unverified. That concern is obsolete: AST-aware remapping has
  been the v8 provider's default since vitest 3.2.0 and is documented as
  producing reports identical to istanbul. The real effect is on the
  denominator — AST remapping counts the functions and branches in Svelte's
  *compiled* output, so generated code inflates the totals substantially.
  One project's vitest 4 upgrade reported a component with no tests going
  from 1 function and 1 branch to 13 functions and 40 branches.

  `diff-cover` scores line coverage on changed lines, so the worst of this
  misses the gate. It will still make `.svelte` percentages in the HTML
  report look far worse than the hand-written code warrants, and it makes
  the `coverage-exempt` label more load-bearing than it appears. The first
  implementation step therefore measured the real starting numbers before
  the threshold was fixed; see **Measured baseline** above.
* **`livtet-plugin-host` and `livtet-sync-daemon` have no tests at all**
  (0/314 and 0/30 lines). They are the Rust counterpart to the untested
  route components: any change to either faces an 85% patch gate with no
  existing test infrastructure to build on. Standing up a first test for
  each is the cheapest way to avoid the exemption label becoming routine
  for sidecar work.
* Coverage percentage is a proxy, not a measure of test quality. It is not
  to be read as one.

## Future extensions

* Reintroduce a badge and trend line if the public number becomes wanted;
  the lcov outputs already support it, at the cost of a `contents: write`
  job.
* Instrument the frontend build to fold Playwright e2e coverage in.
* Add a project-level floor once the baseline is known, ratcheted upward
  deliberately rather than enforced as "must never drop."
