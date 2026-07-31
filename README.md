# Livtet for Desktop

[![CI](https://github.com/olamaelcu/livtet-desktop/actions/workflows/ci.yml/badge.svg)](https://github.com/olamaelcu/livtet-desktop/actions/workflows/ci.yml)

A coherent interface for daily work, shipped as a Tauri desktop app.

Search, browse, and manage a personal library of books. Livtet indexes local and remote catalogues, fetches cover art and edition details, and surfaces metadata through a fast command palette. The frontend is SvelteKit with [WebAwesome](https://webawesome.com/) custom elements and the Geist font family. The shell is Rust, talking to the webview through Tauri IPC.

## Stack

- Tauri 2 with `tauri-plugin-opener`, `tauri-plugin-decorum` (overlay titlebar), and `tauri-plugin-mcp-bridge` in dev
- SvelteKit 2 + Svelte 5, bundled by Vite 8, source under `web/`
- WebAwesome 3.x components, loaded via the auto-loader
- Geist Sans and Geist Mono via `@fontsource-variable`
- Rust 1.85+, edition 2021, single workspace member in `tauri/`

## Quick start

```sh
pnpm install
pnpm tauri:dev
```

See [GETTING_STARTED.md](./GETTING_STARTED.md) for prerequisites, the full dev loop, and how to produce a release build.

## Contributing

Bug reports, fixes, and feature work all flow through pull requests. Read [CONTRIBUTING.md](./CONTRIBUTING.md) for commit conventions, code style, and the review process.

## License

MPL-2.0. See [LICENSE](./LICENSE). Copyright (c) 2026 Jacky Alcine <yo@jacky.wtf>.