# 45. Serve the library over an OPDS 2.0 server

Date: 2026-10-31

## Status

Accepted

## Context

The library lives on the user's machine, and the devices that read it (e-readers,
tablets running a reading app) are not the desktop. Until now the only way to get
a book onto one was to leave the app: find the file on disk and copy it. Readers
already speak OPDS, so a catalog served from the desktop is the standard answer.

The OPDS implementation exists in a separate repository
(`github.com/olamaelcu/livtet-opds`, crate `livtet-opds-server`): a poem-based
route table over an `OpdsRepository` trait, with a `LivtetOpdsRepository` adapter
that reads `livtet-core`'s SeaORM pool and Tantivy index. It has no binary and no
notion of settings; it is a library that starts when told an address.

Alternatives considered:

1. **A separate `livtet-opds` server binary with its own state.** The same
   argument that rejected a separate MCP binary (ADR-0041) applies: it would open
   a second SQLite pool on the same file and a second Tantivy handle, which
   Tantivy refuses (the `IndexWriter` lock is exclusive, and `SearchIndex` cannot
   be reopened to make a second one).
2. **Expose the catalog through the existing loopback byte server.** It serves
   file bytes for the webview, has no catalog model, and binds loopback only —
   nothing an e-reader could browse.
3. **Wait for a full library-sync server.** Sync moves the catalog between Livtet
   clients; an e-reader is not a Livtet client and will never speak the sync
   protocol.

## Decision

Mount `livtet-opds-server`'s `PoemOpdsServer` inside the desktop process, over
the live `AppState`, and give it a Settings tab.

- **Dependency.** `livtet-opds-server` comes in by git URL, beside the
  `livtet-opds-client`/`livtet-opds-types` already used for subscribed catalogs.
- **Wiring.** `livtet-desktop/src/opds_server.rs` owns `OpdsServerRuntime`: the
  persisted settings, the live `PoemOpdsServer`, and the URL its feed links are
  built from. The route table's handler is an adapter (`LiveOpdsHandler`) over
  `OpdsService`, because the crate ships the service but no `OpdsHandler` impl.
- **State.** `AppState::search_index` now holds `Option<Arc<SearchIndex>>`
  instead of `Option<SearchIndex>`. `LivtetOpdsRepository` needs a shared handle,
  and a `SearchIndex` owns the exclusive Tantivy writer, so it cannot be
  reopened; sharing one handle is the only way both the commands and the server
  search the same index.
- **Binding.** Every interface (`0.0.0.0`), unlike the loopback-only MCP endpoint:
  the client is another device on the network, and a loopback-bound OPDS feed has
  nothing to serve. Feed links are absolute, so they are built from the machine's
  primary IPv4 address (falling back to loopback when it has none), not from the
  bind address.
- **Authentication.** None. Anyone who can reach the port can read the library.
  The Settings panel says so, and the server is off until the user turns it on.
- **Activation.** `opds-server.json` holds `{ enabled, port }` (default off, port
  8080). App setup starts the server only when that file says to; a failure is
  logged rather than failing setup. IPC: `opds_server_status`,
  `opds_server_settings_save`, `opds_server_start`, `opds_server_stop`.

## Consequences

- An e-reader on the same network browses and downloads the library with no
  export step, and sees a book the moment it is imported: the server reads the
  same index the importer writes.
- The library is readable by anything on the network while the server is on. That
  is the accepted cost of an unauthenticated OPDS catalog; the alternative (basic
  auth over cleartext HTTP, or a certificate every reader has to trust) buys
  little, and the crate's `AuthConfig` is unused for now.
- The listener is a process-wide resource, so an `opds-server.json` port already
  in use surfaces as a `bind` error in Settings instead of a silent fallback to
  another port.
- `AppState::search_index` changed shape, touching every reader of the index
  (search, bulk, import, calibre, ATProto shelves, the MCP bridge). That is
  mechanical but load-bearing: a second `SearchIndex` on the same directory would
  fail at runtime, not at compile time.
- The published feed is only as good as `LivtetOpdsRepository`'s mapper, whose
  `self` link still points at a placeholder `/opds/v1.2/root` path. Clients that
  follow the root document's `publications`/`navigation` links are unaffected;
  such a client would need that fixed upstream in `livtet-opds`.
