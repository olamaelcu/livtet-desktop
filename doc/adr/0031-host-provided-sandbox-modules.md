# 31. Host-provided sandbox modules (xml, sqlite, http)

Date: 2026-09-29

## Status

Accepted, amended 2026-10-30 (see [Amendment](#amendment-2026-10-30)). Builds on
[ADR-0029](0029-livtet-plugin-host-sidecar.md) and
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

## Amendment (2026-10-30)

The `http` decision above said the fetch runs under "a domain allowlist", which
reads as one list the application supplies. It has two inputs, and the
distinction is the security boundary ([ADR-0046](0046-enrich-edition-metadata-from-plugin-enrichers.md)):

- **The application's own read of the plugin manifest.** `granted_domains_for_plugin`
  parses `<plugins_dir>/<plugin>/plugin.toml` → `[capabilities.http] domains`. This
  is the trust anchor: the app decides what a plugin may reach, from a file on a
  root it owns (ADR-0034).
- **The grant the host forwards with the call.** `CallbackCall.grant` is an untrusted
  hint, because the callback travels from the host process to the app and the child
  chose it. stanchion's own note on `on_callback`
  (`crates/stanchion-remote/src/client.rs:268-277`): "Re-checking `call.grant` proves
  nothing, because the child chose it… treat `call.grant` as an untrusted hint."

`answer_http(call, plugins_dir)` therefore permits the **intersection** of the two,
and refuses every host when either side is empty or when they are disjoint — each with
its own message, so a misconfigured manifest is distinguishable from a host that
forwarded nothing. A plugin name that is not a single path segment reads no manifest,
so `../x` cannot escape the plugins root; unparsable manifests, non-string entries and
blank entries grant nothing. The enricher host client binds the handler with
`on_callback` and restricts `allow_capabilities(["http"])`, so no other capability
reaches it.

This corrects the closing sentence of Consequences. A capability is gated by the
manifest the **application** reads plus the plugin declaring it — not by "host-config
allowlist" — and the forwarded grant only narrows that further. It also makes the
forward-looking clause above concrete: the first `http` client is the enricher, whose
OpenLibrary plugin declares `domains = ["openlibrary.org", "covers.openlibrary.org"]`.
ADR-0030's deferred per-capability grant store would replace the manifest read, not
the intersection rule.
