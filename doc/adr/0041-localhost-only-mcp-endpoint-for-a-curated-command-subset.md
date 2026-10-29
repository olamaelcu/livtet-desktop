# 41. Expose a curated command subset over a localhost-only MCP endpoint

Date: 2026-10-28

## Status

Accepted

## Context

The desktop app drives its library through Tauri IPC: the webview calls a
command, the command runs in the app process against `AppState`. Nothing
outside the app can search the catalog, import a book, or read a publication,
so an external assistant or script has no supported way to work with a local
library.

The Model Context Protocol is the standard shape for that client, and the
official Rust SDK (`rmcp`) ships a Streamable HTTP server transport. The open
questions are which commands to expose, how the endpoint reaches `AppState`,
and what stands in for authentication.

Alternatives considered:

1. **Expose every command.** One router over the whole Specta command list.
   The list includes destructive bulk operations (`delete_editions`),
   plugin installation from a path or remote, sync pairing and device
   revocation, ATProto identity writes, and OPDS acquisition. That is an
   unbounded surface published to a new client with no review step.
2. **A separate `livtet-mcp-server` *binary* with its own state.** A second
   process would need its own `AppState`: a second `SharedState` pool on the
   same SQLite file, a second `SearchIndex` handle (tantivy holds a writer
   lock), a spawned sync daemon, the OS keyring, and the loopback byte server.
   The desktop does not expose any of that incrementally, so the binary would
   either duplicate `app_setup` or proxy every call back to the app — which
   needs an IPC channel that does not exist.
3. **A token / JWT on top of a network bind.** Standard for remote servers,
   but this endpoint is local: there is no user to authenticate, and a secret
   stored next to the library weakens rather than strengthens the boundary.
4. **An in-process MCP server on loopback, holding the live `AppState`, with a
   hand-picked tool list.** Tool calls run the same `*_inner` functions the
   IPC commands call.

## Decision

Add `crates/livtet-mcp-server`, a library crate (not a binary) built on
`rmcp` 3 with the Streamable HTTP server transport, and start it inside the
desktop process.

- **Contract.** `DesktopBridge` (in `livtet-mcp-server/src/bridge.rs`) mirrors
  the exposed command signatures, returning serialized JSON. The crate links
  neither Tauri nor the catalog model, so the tool surface is testable against
  a stub.
- **Implementation.** `livtet-desktop/src/mcp.rs` implements `DesktopBridge`
  over the live `AppState` and calls the same `search_*_inner`, `import_*`,
  and reader functions the IPC commands use. No second process, no socket
  bridge, no second connection pool.
- **Allowlist.** The tool router *is* the list: one `#[tool]` method per
  exposed command in `tools.rs`. Published tools: `search_typeahead`,
  `search_editions`, `filter_options`, `import_file`, `import_files`,
  `create_virtual_book`, `reader_publication`, `reader_resource`,
  `get_reader_position`, `save_reader_position`, `list_reader_bookmarks`,
  `add_reader_bookmark`, `remove_reader_bookmark`. Deliberately absent:
  `open_reader` (it opens a window — a UI act with no meaning to a headless
  client), bulk delete/export, plugin installation, OPDS acquisition, sync
  pairing, and every ATProto identity write.
- **Binding.** `127.0.0.1` only, never a wildcard address, and the transport
  keeps its default loopback `Host` allowlist so a forged `Host` (the
  DNS-rebinding path a browser page could take) is answered with 403.
- **Authentication.** None. The bind address is the boundary; a remote caller
  tunnels over SSH, which is already an authenticated channel.
- **Activation.** Opt-in: the endpoint binds only when `LIVTET_MCP_PORT` is
  set. An empty value means `DEFAULT_PORT` (7777); any other value must be a
  port, and one that is not leaves the endpoint off. A normal launch opens no
  socket.
- **Transport shape.** `json_response` is enabled, so a request/response tool
  answers with JSON rather than an SSE stream; a tool that emitted a
  notification would still fall back to SSE, so nothing is lost.

## Consequences

- A local client can search, import, and read a library without the webview,
  while every call runs the reviewed command code paths.
- The endpoint is reachable by any local process under the same user. On a
  shared machine that means a shared library; `import_file`/`import_files`
  write catalog rows and index documents, so a local client can mutate the
  library. This is the accepted cost of not inventing a token.
- The published tool names are pinned by
  `crates/livtet-mcp-server/tests/endpoint.rs`, so a tool can neither vanish
  nor appear unnoticed, and the forged-`Host` guard is asserted there too.
- No standalone binary exists. The plan's sidecar option was rejected because
  a second process cannot reach `AppState` without duplicating `app_setup`
  (DB pool, search index lock, sync daemon, keyring); if a detached server is
  ever needed, the bridge trait is the seam to add a transport to, not a
  reason to fork the state.
