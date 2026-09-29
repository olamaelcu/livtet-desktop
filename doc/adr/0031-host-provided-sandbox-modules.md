# 31. Host-provided sandbox modules (xml, sqlite, http)

Date: 2026-09-29

## Status

Accepted. Builds on [ADR-0029](0029-livtet-plugin-host-sidecar.md) and
[ADR-0030](0030-plugin-management-ipc.md).

## Context

Plugins need common facilities — parsing XML, querying SQLite, making HTTP
requests — that they would otherwise pull from LuaRocks. Native LuaRocks
packages are C extensions: loading one runs arbitrary native code, bypassing the
sandbox and capability model, and breaks per-platform signing. So Livtet
provides these as host capabilities instead.

## Decision

Register three capabilities the sandbox exposes to plugins that declare them:

- **`xml`** (in-host) — a module `{ parse(source) -> node }` backed by
  `livtet_epub::xml`'s tolerant parser. Pure computation, so it runs in
  `livtet-plugin-host`.
- **`sqlite`** (in-host) — `query(path, sql, params) -> rows`, opened
  `READ_ONLY` and restricted to the database paths the application granted via
  `livtet-plugin-host --sqlite <path>`. Runtime scoping lives in the launch
  arg because manifest grants are static and the target DB is chosen at runtime.
- **`http`** (app-mediated) — forwarded to `livtet-desktop`
  (`commands/plugin_host_callbacks.rs`), which performs the fetch under a domain
  allowlist with size/time caps and returns `{ status, headers, body }`. It is
  app-mediated so policy and (later) credentials stay on the app's side; the
  plugin never touches the socket.

In-host registration required stanchion's `Registry::with_setup` to be
**additive** (a second `with_setup` now accumulates rather than replaces), landed
upstream (`jalcine/stanchion`) and picked up by bumping the pin. `livtet-plugin-host`
chains a `with_setup` onto `build_registry` to add `xml`/`sqlite`; nothing in
livtet reimplements stanchion internals.

## Consequences

- Easier: plugins stay pure-Lua, sandboxed, signable, and cross-platform; the
  Calibre plugin (ADR-0032) uses `sqlite`, and metadata providers will use `http`.
- Harder: `livtet-plugin-host` gains `quick-xml`/`rusqlite` (via `livtet-epub`
  and directly); `xml`/`sqlite` surface as function-bearing tables while `http`
  is a function. A capability is still gated by policy (host-config allowlist +
  the plugin declaring it).
