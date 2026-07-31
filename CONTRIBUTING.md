# Contributing

Thanks for working on `livtet-desktop`.

## Ground rules

- Open an issue first for non-trivial changes. Bug fixes and small tweaks can go straight to a pull request.
- Keep pull requests scoped. One concern per PR.
- All contributions fall under the [MPL-2.0](./LICENSE) terms. By submitting a contribution you affirm the [Developer Certificate of Origin](https://developercertificate.org/) (signed-off-by in your commit is the easiest way to do this).
- Be kind in review. Critique code, not people.

## Commit conventions

The repo follows [Conventional Commits](https://www.conventionalcommits.org/) with lowercase scopes where useful:

- `feat:` user-visible capability
- `fix:` bug repair
- `refactor:` internal change with no behavior shift
- `chore:` tooling, dependencies, repo hygiene
- `docs:` documentation only
- `style:` formatting only

Keep the subject under 72 characters, imperative mood ("add", not "added"), and no trailing period. Add a body when the why is not obvious from the diff.

## Code style

**TypeScript and Svelte.** The SvelteKit defaults apply: 2-space indent, single quotes, no semicolons in component `<script>` blocks. Run `pnpm check` and `pnpm lint` before pushing; CI runs the same.

**Rust.** Standard `rustfmt` output plus `clippy` defaults. Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo-machete` locally (or `mise run lint`). The workspace targets Rust 1.85 with edition 2021.

**Naming.** Match what already exists in the file. Don't introduce a new convention in a one-line change.

## Frontend conventions

- Use `<wa-*>` custom elements directly. Add new components to `web/app.wa.ts` so the auto-loader picks them up at app boot.
- App styles live under `@layer app` in `web/app.css` so WebAwesome's internal cascade layers don't shadow them.
- Tauri commands live in `tauri/src/lib.rs`. New commands need both a Rust handler and a matching permission in `tauri/capabilities/default.json`.

## Tests

The repo ships with inline Rust tests in `tauri/src/commands/**` and a Vitest runner wired up for the frontend. Both run as part of `mise run test`, which the GitHub Actions CI also invokes.

- Frontend logic: prefer small Svelte 5 runes over extracting testable helpers, but write a Vitest unit test for any pure function worth sharing. Tests live next to the code as `*.test.ts`.
- Tauri commands: integration tests go in `tauri/tests/` (none yet) and inline `#[test]` / `#[tokio::test]` modules next to the command implementation.

Run the full suite locally with `mise run ci`. CI runs the same command on every PR and every push to `main`.

Live HTTP search tests (Hardcover, Google Books, OpenLibrary) are marked `#[ignore]` so CI doesn't hit external APIs. Opt in with `cargo test -- --ignored` when you have API keys.

## Pull request process

1. Branch from `main`.
2. Run `pnpm check` and `cargo clippy --workspace --all-targets` locally. Both must be clean.
3. Push your branch and open a pull request. Fill in the template. Link the issue it addresses.
4. Wait for review. Expect at least one round of comments before merge.
5. Squash or rebase before merge if the maintainer asks; otherwise a merge commit is fine.

## Release process

Releases are tagged from `main` by the maintainer. The version in `package.json` and `tauri/Cargo.toml` must move together. The release notes pull from commit subjects since the previous tag, so well-formed Conventional Commit subjects pay off here.