# Glossary

Single source for livtet-desktop terms. One entry per concept. See the `glossary` skill for lookup/add/audit workflow.

Naming policy: use the canonical term from code or ADRs. Expand abbreviations on first use. Keep definitions short.

## UI

- **Settings** — the `/settings` route hosting the sync panel: server status and start/stop, pairing, paired devices, conflicts, and the recent-request feed. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).

## IPC

- **import_file** — the Tauri command that imports a book through the importer selected for its extension. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **sync_\*** — the Tauri command family (`sync_health`, `sync_status`, `sync_pairing_*`, `sync_devices_*`, `sync_conflicts_*`, `sync_server_*`) proxying the sync control channel. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **sync://…** — the Tauri events the app re-emits from the daemon's notifications: `sync://pairing-requested`, `sync://request`, `sync://completed`, `sync://server-started`, `sync://server-stopped`. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).

## Backend

- **sync daemon** — the `livtet-sync-daemon` sidecar Tauri launches; it calls `livtet_sync_server::run` and hosts the poem `/sync/*` server. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **sync control channel** — JSON-RPC 2.0, newline-delimited, over the daemon's stdin/stdout: requests (`health`, `status`, `requests.recent`, `pairing.*`, `devices.*`, `conflicts.*`, `server.*`, `shutdown`) plus notifications. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **client migration** — the `livtet-data` migrator (`Kind::Client`) that owns `change_log`, `conflicts`, the pairing tables, `client_settings`, and the audit triggers. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **request log** — the daemon's bounded ring buffer of recently served `/sync/*` requests, read via `requests.recent` and refreshed by `sync://request`.
- **fs_read** — host-to-application capability callback that returns the selected import file's bytes, scoped to that exact path. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **plugin host** *(stale — crate removed in the hard-reset; only Rust remnants remain)* — the `livtet-plugin-host` sidecar that ran Lua importers out of process. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).

## Domain

- **change_log** — append-only audit table the trigger set writes on every syncable-table mutation; the source of `SyncChange` rows. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **conflict** — a `conflicts` row recorded when a pushed change clashes with local state; resolved as `local`, `remote`, or `merged`. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **pairing** — the flow that mints a single-use token, is approved on the desktop, and yields a paired device and a session token.
- **Importer** — a file-format plugin implementing the `livtet-importer` contract (`extensions`, `read_metadata`); native EPUB in-app or remote Lua in the plugin host. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **ImporterMeta** — the serde wire record an importer returns (title, contributors, ISBNs, cover). See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **SyncEngine** — the `livtet-sync` domain engine that reads and writes `change_log` and `conflicts`. See [core ADR 1](../../core/docs/adr/0001-split-sync-into-domain-transport-daemon-crates.md).
- **SyncSession** — the app-facing `livtet-sync-http` client composing a `SyncEngine` with a `SyncHttpClient` transport. See [core ADR 1](../../core/docs/adr/0001-split-sync-into-domain-transport-daemon-crates.md).
