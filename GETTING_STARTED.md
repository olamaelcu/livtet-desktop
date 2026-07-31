# Getting Started

Run, edit, and ship `livtet-desktop`.

## Prerequisites

- Node.js 20 or newer (for `pnpm` and the Vite dev server)
- pnpm 9 or newer
- Rust 1.85 or newer (`rustup default stable`)
- Platform dependencies for Tauri 2: see the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for your OS. On Linux that means `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, and a working `pkg-config`.

## Install

```sh
pnpm install
```

This installs both the JS toolchain and pulls in `tauri-cli` as a dev dependency.

## Develop

```sh
pnpm tauri:dev
```

Tauri spawns `pnpm dev` (Vite on `http://localhost:1420`) and launches the desktop window pointed at it. Hot reload works for the Svelte side; Rust edits trigger a recompile and a window restart.

For frontend-only iteration without the webview:

```sh
pnpm dev
```

Type-check Svelte and TypeScript in watch mode:

```sh
pnpm check:watch
```

## Type-check and lint

Run `svelte-check` over the SvelteKit project:

```sh
pnpm check
```

Run `cargo check` against the Rust workspace from the repo root:

```sh
cargo check --workspace
```

## Build a release bundle

```sh
pnpm tauri:build
```

The output lands in `tauri/target/release/bundle/` for your platform (`.dmg` on macOS, `.msi`/`.exe` installers on Windows, `.deb`/`.AppImage` on Linux). The build runs `pnpm build` first to produce the static frontend in `build/`, which Tauri then packages.

## Project layout

```
.
├── web/           SvelteKit frontend (Svelte 5 + WebAwesome)
├── tauri/         Rust crate, Tauri config, capabilities, icons
├── package.json   JS toolchain and Tauri CLI
├── Cargo.toml     Rust workspace root
├── mise.toml      Dev toolchain and task runner
└── vite.config.js Vite + SvelteKit config
```

## CI and local checks

GitHub Actions runs the same commands you run locally, exposed as mise tasks:

```sh
mise run lint   # rustfmt + clippy + cargo-machete + biome
mise run test   # cargo test --workspace --all-targets + vitest
mise run ci     # both, in dependency order — mirrors CI exactly
```

Before pushing a PR, run `mise run ci` from the repo root. The tasks pick up the `pnpm` and `cargo-machete` versions pinned in `mise.toml`, so the local and remote runs use the same toolchain.

## Troubleshooting

**The window opens but stays blank.** Vite is probably not serving on `127.0.0.1:1420`. Confirm `pnpm dev` is running and reachable, then restart `pnpm tauri:dev`.

**Rust changes do not recompile.** Kill the `pnpm tauri:dev` process and run `cargo clean -p tauri` if the build is wedged on stale artifacts.

**`<wa-*>` components render as plain HTML.** The WebAwesome auto-loader in `web/routes/+layout.svelte` hydrates them after mount. If a component never upgrades, check that it is imported in `web/app.wa.ts`.

**MCP bridge can't connect.** The bridge plugin is wired in debug builds only (`tauri/src/lib.rs` gates it behind `#[cfg(debug_assertions)]`). Build a debug binary with `pnpm tauri:dev` rather than running a release build.

## Next steps

- Browse the source: the greeting flow in `web/routes/+page.svelte` calling `invoke("greet")` is the smallest end-to-end example.
- Read [CONTRIBUTING.md](./CONTRIBUTING.md) before opening a pull request.