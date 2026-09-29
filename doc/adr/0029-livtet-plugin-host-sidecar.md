# 29. Ship `livtet-plugin-host` as a Tauri-managed sidecar

Date: 2026-09-28

## Status

Accepted.

## Context

The desktop app already resolves a `livtet-plugin-host` binary
(`crates/livtet-desktop/src/lib.rs`, `resolve_plugin_host`) and drives it
through `stanchion::remote::RemoteRegistry` (`commands/plugins.rs`,
`commands/importers.rs`). The binary itself was never in the workspace —
`resolve_plugin_host` only warned when it was missing, so `list_plugins` had no
host to launch and the `commands::importers` remote tests failed for lack of the
binary. [ADR-0012](0012-embedded-sync-daemon-and-jsonrpc-control.md) notes an
earlier plugin host established the sidecar pattern here before it was removed.

## Decision

Add a `crates/livtet-plugin-host` binary crate: a thin wrapper over
`stanchion::remote::{load_config, build_registry, serve, HostChannel}`, the same
shape as stanchion's own `plugin-host` reference binary. The host is
**contract-agnostic** — it loads whatever plugins live under the plugins root and
answers method calls by name. The importer contract (`extensions`,
`read_metadata`) and the `report` listing are enforced by the caller in
`livtet-desktop`, so the host does not depend on `livtet-importer` or register
`ImporterClass`.

Stage it like the sync daemon: `crates/livtet-desktop/build.rs` nested-builds
each sidecar into its own target dir and copies it to `binaries/<name>-<triple>`
(for `bundle.externalBin`) and next to the app binary (for dev). The daemon-only
staging block was refactored into a `stage_sidecar` helper called for both
`livtet-sync-daemon` and `livtet-plugin-host`.

## Consequences

- Easier: `list_plugins` and remote importer plugins now have a host to launch;
  the previously-failing `commands::importers` tests pass. Bundled installs ship
  the host via `externalBin`.
- Harder: every `livtet-desktop` build now nested-builds two sidecars, each
  compiling stanchion (vendored Lua) into an isolated target dir — more build
  time. Mitigated by `cargo:rerun-if-changed` on each crate dir.
- The host is generic: a plugin missing a called method fails at call time, not
  load time — already handled by the caller (`commands/importers.rs`).
