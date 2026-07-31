# 6. Continuous integration via mise tasks and GitHub Actions

Date: 2026-07-31

## Status

Accepted

## Context

`livtet-desktop` had no CI pipeline. Tests existed but were never
automatically verified. Linting was partial (cargo fmt, rustfmt config
missing). Documentation was ungeneratable (cargo doc failed on missing
prost-types). A working CI would guard merges to main.

## Decision

Instrument the repository with GitHub Actions, driven by mise tasks that
mirror the CI locally and in CI.

### Mise tasks

Add three tasks to `mise.toml`:

* `lint` — run cargo fmt `--check`, clippy, cargo-machete to detect unused
  deps, and biome to lint/format frontend code.
* `test` — run cargo test and `pnpm vitest test` with `--run`.
* `ci` — depend on both `lint` and `test`.

### GitHub Action

Create `.github/workflows/ci.yml` that:

* Installs mise (mise.run) and caches pnpm/Rust toolchain.
* Runs `mise exec -- pnpm ci` for dependency install.
* Decrypts `.mise/secrets.json.sops` via SOPS age key.
* Executes `mise run ci` as the final step.

### Tooling decisions

* **Biome 2.x** — Added `biome.json` with Svelte-specific overrides for
  unused imports/variables, which cannot be resolved by Biome's Svelte
  analysis. Template references (`bind:this`, reactive statements) trigger
  false positives.

* **Vitest 2.x** — Added `vitest.config.ts`. Tests live in `web/*.test.ts`
  alongside source. Use `--passWithNoTests` because CI should pass for
  crates with no corresponding frontend tests.

* **cargo-machete 0.7** — Detect unused dependencies. The flag
  `--install-path` from 0.6.x was removed; run as standalone binary.

* **cargo fmt + clippy** — Standard Rust formatting and linting, invoked
  via mise task for consistency.

### Secrets integration

Extend ADR 4 by providing the CI age keypair to `SOPS_AGE_KEY` GitHub
secret. CI decrypts `.mise/secrets.json.sops` and exports values via the
existing `secrets-export-env` task before `secrets.json` becomes available
to `tauri/build.rs`.

## Deviations from initial implementation

Several pragmatic corrections were made after initial planning:

* `cargo-machete --install-path` (aqua CLI pattern) does not exist in
  0.7.x; use binary directly instead of cargo subcommand pattern.

* `biome.json` `usesTs` must be `true` for `.svelte` files containing
  TypeScript (`<script lang="ts">`).

* `pnpm=10` fixed in mise.toml after initial `pnpm=9` caused test
  dependency resolution failures.

* `aqua:bnjbvr/cargo-machete` not in aqua registry; use `cargo:cargo-machete`
  backend instead.

* `--unsafe` Biome format corrupts Svelte templates; add `.svelte` overrides
  instead of global `--unsafe`.

* `web/lib/bindings.ts` must be force-ignored by Biome; its shape is derived
  from Rust enums and cannot follow frontend import conventions.

* Live HTTP tests (Hardcover, Google Books, OpenLibrary) marked `#[ignore]`
  to avoid transient network failures in CI.

* `cargo doc` failed on missing `prost-types`; restored `prost = "=0.13.4"`
  feature scope to fix offline doc generation.

* Taskwarrior/mcp tools blocked mise installation in CI; disabled via
  `MISE_DISABLE_TOOLS=taskwarrior,pipx:taskwarrior-mcp` env var.

## Consequences

### Becomes easier

* Developers can run `mise run lint` and `mise run test` with the same
  commands CI uses.

* CI can evolve by editing `mise.toml` without touching the workflow file.

* Unused dependencies surface early; `cargo-machete` runs in pre-commit
  hooks or CI.

### Becomes harder

* New contributors must learn mise to run tests locally; docs updated.

* Biome cannot auto-fix certain Svelte patterns; developers must lint
  manually or use targeted fixes.

## Future extensions

* **Matrix testing** — Add Rust/MSVC job for Windows compatibility;
  enable cross-platform testing.

* **Caching improvements** — Cache `cargo-machete` binary and `.cargo/bin`
  separately from full rustup toolchain.

* **Additional lint tools** — Consider `cargo deny` for licensing,
  `cargoaudit` for CVEs.

* **Pre-commit hooks** — Install mise globally and add pre-commit config
  invoking `mise run pre-commit`.

* **Release automation** — CI can build and sign binaries, attach to GitHub
  releases.