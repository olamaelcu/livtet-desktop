# 12. Embed the sync server as a Tauri-managed sidecar controlled over JSON-RPC

Date: 2026-09-23

## Status

Accepted

## Context

The desktop has to host the sync protocol so paired devices can sync against the
desktop library, and the UI needs to see health, sync status, incoming requests,
pairings, and conflicts.

Three constraints shaped the decision:

1. **The protocol server is a separate concern from the UI.** Running the poem
   server inside the Tauri runtime would entangle its lifecycle with the app's,
   and a panic or OOM in the server would take the whole application down.
2. **The core already exposes a reusable server.** `livtet-sync-server` (core)
   provides `run(ServerConfig)` and its own binary; the desktop should not
   reimplement it.
3. **The desktop must drive and observe it.** A control channel was needed for
   `health`, `status`, an incoming-request feed, pairing approval, and device
   management — and for the server to push events (a device requesting to pair,
   incoming sync traffic).

The previous out-of-process plugin host had established the sidecar pattern in
this codebase, but it was removed; the sync daemon is the only sidecar now.

## Decision

Ship the sync server as a **Tauri-managed sidecar**:

* **Binary** — `crates/livtet-sync-daemon` is a thin wrapper whose `main()`
  parses `ServerConfig` and calls `livtet_sync_server::run`. It is staged by
  `crates/livtet-desktop/build.rs` using `escargot` (a nested `cargo build` into
  an isolated target dir) and declared in `bundle.externalBin`, so Tauri
  packages it next to the application binary.
* **Launch** — via `tauri-plugin-shell`'s `ShellExt::shell().sidecar(
  "livtet-sync-daemon")`, so Tauri owns path resolution and process lifetime.
  The daemon is started in `app_setup` with `--db <data>/livtet.db --host
  127.0.0.1 --port 0`.
* **Control protocol** — **JSON-RPC 2.0, newline-delimited JSON over
  stdin/stdout**. The daemon's stdout carries only NDJSON; its logs go to
  stderr. The codec and method-name constants live in
  `livtet_sync_server::rpc` and are shared by both sides.
  * Requests: `health`, `status`, `requests.recent`, `pairing.begin|list|
    approve|reject`, `devices.list|revoke`, `conflicts.list|resolve`,
    `server.start|stop`, `shutdown`.
  * Notifications: `server.started`, `server.stopped`, `request.received`,
    `pairing.requested`, `sync.completed`; the desktop re-emits them as Tauri
    events (`sync://…`).
* **Database** — the daemon opens the same `livtet.db` as the app. WAL plus
  `busy_timeout = 5000` make concurrent access safe; writes serialize.
* **Schema** — `livtet-data`'s client migrations own `change_log`, `conflicts`,
  and the audit triggers; the desktop connects with `Business` + `Client`.

## Consequences

### Easier

* The server's lifecycle is independent: a crash is contained, and the app can
  relaunch it.
* The IPC surface is a small, typed contract in core; the desktop mirrors it
  with specta commands (`sync_*`) and result DTOs.
* The UI can react to server-pushed events without polling.

### Harder / to watch

* **Two processes, one SQLite file.** WAL + busy timeout cover it, but the same
  database is written from both; monitor for lock contention.
* **Build cost.** `build.rs` runs a nested cargo build for the daemon into a
  separate target dir, roughly doubling the daemon's compile time; source
  changes in core crates do not trigger a restage unless `build.rs` reruns.
* **Local-dev dependency override.** Until core is pushed, the desktop
  workspace's `livtet-*` dependencies point at `../core` paths and must be
  switched back to a git rev.

### Deferred

* **Authentication.** No `/sync/*` route validates a session token, and
  `get_file` serves any inventory by hex id. The server binds `127.0.0.1` by
  default; this must be fixed before exposing it on a LAN or the internet.
* Desktop acting as a sync client (pull/push against a remote server).
* Binary file/cover sync.
* `Conflict::id` type mismatch between `list_conflicts` (`DbId`) and
  `resolve_conflict` (`i64`).
