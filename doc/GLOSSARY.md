# Glossary

Single source for livtet-desktop terms. One entry per concept. See the `glossary` skill for lookup/add/audit workflow.

Naming policy: use the canonical term from code or ADRs. Expand abbreviations on first use. Keep definitions short.

## UI

- **Settings** — the `/settings` route hosting the sync panel: server status and start/stop, pairing, paired devices, conflicts, and the recent-request feed. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **Library toolbar** — the action row above the library search box; today it hosts **Add book**. See [ADR 0014](adr/0014-batch-file-import-with-progress-events.md).
- **add-book drawer** — the right-hand `wa-drawer` that picks or receives dropped EPUB/AZW3/AZW/PDF files and shows per-file import progress.
- **edition-detail drawer** — the right-hand `wa-drawer` opened by clicking a book in the library; shows that edition's cover, contributors, publishers, identifiers, and file. See [ADR 0015](adr/0015-add-get-edition-detail-ipc-and-edition-detail-drawer.md).
- **Filter panel** — the popover that narrows the library by availability, format, language, author, tag, genre, subject, and publisher, and sets sort order. See [ADR 0016](adr/0016-filtered-library-search-ipc.md).
- **Availability filter** — the filter panel's tri-state (Any / In filesystem / Not in filesystem) control, backed by `EditionFilters.has_file`. See [ADR 0021](adr/0021-file-availability-filter-and-card-badges.md).
- **File-availability badge** — the paired badge on a library card: `hard-drive` when the edition's file is on disk, `cloud` when it is virtual or remotely referenced. See [ADR 0021](adr/0021-file-availability-filter-and-card-badges.md).
- **Selection mode** — the library state where cards show checkboxes and clicks toggle selection. See [ADR 0017](adr/0017-bulk-edition-mutations.md).
- **Selection action bar** — the toolbar that appears in selection mode with Tag, Export CSV, Delete, and Clear. See [ADR 0017](adr/0017-bulk-edition-mutations.md).

## IPC

- **get_edition_detail** — the read-only Tauri command returning one edition's full catalog record (`EditionDetail`), or `null` when absent. See [ADR 0015](adr/0015-add-get-edition-detail-ipc-and-edition-detail-drawer.md).
- **get_edition_covers** — the read-only Tauri command resolving cover paths for a batch of editions in one indexed lookup (`EditionCover[]`; editions without a cover are omitted). See [ADR 0015](adr/0015-add-get-edition-detail-ipc-and-edition-detail-drawer.md).
- **import_file** — the Tauri command that imports a book through the importer selected for its extension. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **import_files** — the batch Tauri command that imports a list of paths and returns an `ImportBatchResult`. See [ADR 0014](adr/0014-batch-file-import-with-progress-events.md).
- **import://batch**, **import://file** — the Tauri events carrying batch and per-file import progress. See [ADR 0014](adr/0014-batch-file-import-with-progress-events.md).
- **sync_\*** — the Tauri command family (`sync_health`, `sync_status`, `sync_pairing_*`, `sync_devices_*`, `sync_conflicts_*`, `sync_server_*`) proxying the sync control channel. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **sync://…** — the Tauri events the app re-emits from the daemon's notifications: `sync://pairing-requested`, `sync://request`, `sync://completed`, `sync://server-started`, `sync://server-stopped`. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **EditionFilters** — the desktop filter DTO accepted by `search_editions` / `search_editions_count`; carries the id axes, `has_file` availability, and sort. See [ADR 0016](adr/0016-filtered-library-search-ipc.md) and [ADR 0021](adr/0021-file-availability-filter-and-card-badges.md).
- **filter_options** — the command returning filterable values per axis. See [ADR 0016](adr/0016-filtered-library-search-ipc.md).
- **delete_editions**, **export_editions_csv**, **add_edition_tags**, **remove_edition_tags**, **matching_edition_ids** — the bulk edition commands; errors are `BulkError`. See [ADR 0017](adr/0017-bulk-edition-mutations.md).
- **ImportMode** — the per-batch import storage mode (`link` | `copy`) accepted by `import_file` / `import_files`; `link` (default) symlinks the source into the library, `copy` duplicates its bytes. See [ADR 0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).
- **relink_edition_file** — the Tauri command that repoints a library file at a newly picked source, updating the edition's file identity. See [ADR 0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).

## Backend

- **sync daemon** — the `livtet-sync-daemon` sidecar Tauri launches; it calls `livtet_sync_server::run` and hosts the poem `/sync/*` server. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **sync control channel** — JSON-RPC 2.0, newline-delimited, over the daemon's stdin/stdout: requests (`health`, `status`, `requests.recent`, `pairing.*`, `devices.*`, `conflicts.*`, `server.*`, `shutdown`) plus notifications. See [ADR 0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md).
- **client migration** — the `livtet-data` migrator (`Kind::Client`) that owns `change_log`, `conflicts`, the pairing tables, `client_settings`, and the audit triggers. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **request log** — the daemon's bounded ring buffer of recently served `/sync/*` requests, read via `requests.recent` and refreshed by `sync://request`.
- **fs_read** — host-to-application capability callback that returns the selected import file's bytes, scoped to that exact path. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **plugin host** *(stale — crate removed in the hard-reset; only Rust remnants remain)* — the `livtet-plugin-host` sidecar that ran Lua importers out of process. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **livtet-mobi** — the in-crate parser extracting MOBI-family metadata and covers; no text decompression. See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **MobiImporter** — the native `livtet-importer` implementation backing `.azw3` and `.azw`. See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **books_dir** — the library-owned file store (`{app_dir}/data/books`), held on `Paths` and `AppState`; every import materializes one `{sha256}-{original_filename}` entry here. See [ADR 0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).
- **library file** — the symlink or copy inside `books_dir` that `digital_inventory.file_path` points at after import; deleting it never touches the source. See [ADR 0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).
- **missing file** — a library file whose link target no longer exists (`EditionFile.file_status == Missing`); shown with a warning and a re-link action in the edition detail drawer. See [ADR 0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).

## Domain

- **AZW3 / KF8** — the Kindle Format 8 ebook format: a MOBI container (`file_version` 8) holding KF8 markup; imported from `.azw3`. See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **MOBI** — the Mobipocket/Kindle container family (PalmDB records plus PalmDOC/MOBI headers and EXTH); `.azw` and `.azw3` are instances. See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **PalmDB (PDB)** — the record-database container underlying every MOBI/AZW/AZW3 file: a 78-byte header plus an offset table of records. See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **EXTH** — the extended header block in MOBI record 0 carrying bibliographic metadata (title, authors, publisher, ISBN, cover offset). See [ADR 0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md).
- **change_log** — append-only audit table the trigger set writes on every syncable-table mutation; the source of `SyncChange` rows. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **conflict** — a `conflicts` row recorded when a pushed change clashes with local state; resolved as `local`, `remote`, or `merged`. See [core ADR 2](../../core/docs/adr/0002-own-the-sync-client-schema-in-livtet-data-migrations.md).
- **pairing** — the flow that mints a single-use token, is approved on the desktop, and yields a paired device and a session token.
- **Importer** — a file-format plugin implementing the `livtet-importer` contract (`extensions`, `read_metadata`); native EPUB and MOBI in-app or remote Lua in the plugin host. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **ImporterMeta** — the serde wire record an importer returns (title, contributors, ISBNs, non-ISBN identifiers, cover). A title and at least one contributor are required; ISBNs are optional. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md) and [ADR 0020](adr/0020-isbn-optional-for-epub-imports-with-a-body-text-fallback.md).
- **SyncEngine** — the `livtet-sync` domain engine that reads and writes `change_log` and `conflicts`. See [core ADR 1](../../core/docs/adr/0001-split-sync-into-domain-transport-daemon-crates.md).
- **SyncSession** — the app-facing `livtet-sync-http` client composing a `SyncEngine` with a `SyncHttpClient` transport. See [core ADR 1](../../core/docs/adr/0001-split-sync-into-domain-transport-daemon-crates.md).
