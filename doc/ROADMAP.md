# Project Roadmap

The goals of this project are as follows:

- provide a local library experience
- read books and listen to audiobooks in-app
- sync a library across devices
- source metadata and files through signed plugins
- allow for custom theming of the library interface
- local file management and cover management of books

## Status legend

`[x]` done · `[~]` in progress · `[ ]` backlog

> **Layout note.** The Rust code lives in a Cargo workspace under
> `crates/` (`livtet-desktop`, `livtet-epub`, `livtet-mobi`,
> `livtet-pdf`, `livtet-audio`, `livtet-reader`, `livtet-importer`,
> `livtet-importer-types`, `livtet-sync-daemon`). Tauri commands are in
> `crates/livtet-desktop/src/commands/`. The SvelteKit frontend is under
> `web/` (`web/routes/`, `web/lib/`). Earlier revisions of this document
> referenced a `tauri/src/commands/` tree that no longer exists.

## Implemented

### Search & metadata

- [x] Plugin-backed provider architecture — providers are supplied by
      signed plugins through a stanchion registry rather than hardcoded
      Rust `Provider` impls. `crates/livtet-desktop/src/commands/plugins.rs`
      (`stanchion::remote::RemoteRegistry`),
      `crates/livtet-desktop/src/commands/search.rs`.
- [x] Filtered library search IPC —
      [ADR-0016](adr/0016-filtered-library-search-ipc.md),
      `commands/search.rs`.
- [x] Edition detail IPC + drawer —
      [ADR-0015](adr/0015-add-get-edition-detail-ipc-and-edition-detail-drawer.md),
      `web/lib/library/EditionDetailDrawer.svelte`,
      `web/routes/catalog/[catalogId]`.
- [x] Filter-option decorations + remote publisher logos —
      [ADR-0019](adr/0019-expose-filter-option-decorations-and-allow-remote-publisher-logos.md).

### Library & catalog

- [x] Library route — grid of `BookCard`s, add-book drawer, tag picker,
      confirm dialog. `web/routes/library/`, `web/lib/library/`.
- [x] File-availability filter + card badges —
      [ADR-0021](adr/0021-file-availability-filter-and-card-badges.md).
- [x] Bulk edition mutations —
      [ADR-0017](adr/0017-bulk-edition-mutations.md), `commands/bulk.rs`.
- [x] `digital_inventory` 1:1 with editions (UNIQUE) + `edition_files`
      1:N —
      [ADR-0005](adr/0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md).

### Import

- [x] Batch file import with progress events —
      [ADR-0014](adr/0014-batch-file-import-with-progress-events.md),
      `commands/import.rs`.
- [x] Importer plugin contract + `import-file` command —
      [ADR-0011](adr/0011-importer-plugin-contract-and-import-file-command.md),
      `crates/livtet-importer`, `crates/livtet-importer-types`.
- [x] EPUB import via an in-crate OCF/OPF parser —
      [ADR-0009](adr/0009-epub-import-pipeline.md),
      [ADR-0018](adr/0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md),
      [ADR-0020](adr/0020-isbn-optional-for-epub-imports-with-a-body-text-fallback.md),
      `crates/livtet-epub`.
- [x] MOBI-family import via an in-crate parser —
      [ADR-0023](adr/0023-import-mobi-family-files-with-an-in-crate-livtet-mobi-parser.md),
      `crates/livtet-mobi`.
- [x] Native PDF import (pdf-oxide + hayro) —
      [ADR-0027](adr/0027-native-pdf-import-via-pdf-oxide-and-hayro.md),
      `crates/livtet-pdf`.
- [x] Audiobook import —
      [ADR-0025](adr/0025-audiobook-import-and-playback.md),
      `crates/livtet-audio`.
- [x] Language-code normalization with isolang —
      [ADR-0007](adr/0007-normalize-language-codes-with-isolang-during-import.md).
- [x] Library-owned book files: symlink by default, copy opt-in —
      [ADR-0022](adr/0022-library-owned-book-files-symlink-default-copy-opt-in.md).

### Reader

- [x] Unified reader dispatching EPUB + audiobook backends —
      `commands/reader.rs`, `crates/livtet-reader`.
- [x] Readium navigator with an in-app EPUB streamer —
      [ADR-0028](adr/0028-adopt-readium-navigator-with-an-in-app-epub-streamer.md),
      `web/routes/reader/pub/[editionId]`.
- [x] Audiobook playback over loopback HTTP —
      [ADR-0025](adr/0025-audiobook-import-and-playback.md),
      `commands/audio_server.rs`, `commands/playback.rs`,
      `web/routes/reader/audio/[editionId]`.

### Covers

- [x] Hash-keyed cover storage with provider-backed fetchers —
      [ADR-0008](adr/0008-hash-keyed-file-cover-storage-with-provider-fetchers.md),
      `commands/catalog.rs` (`cover_path`).

### Sync

- [x] Embedded sync daemon + JSON-RPC control —
      [ADR-0012](adr/0012-embedded-sync-daemon-and-jsonrpc-control.md),
      `crates/livtet-sync-daemon`, `commands/sync.rs`,
      settings → Sync panel (`web/lib/components/SyncPanel.svelte`).

### OPDS

- [x] In-app OPDS catalog browsing + acquisition —
      [ADR-0026](adr/0026-in-app-opds-catalog-browsing-and-acquisition.md),
      `commands/opds.rs`.
- [x] OS keyring for OPDS credentials —
      [ADR-0024](adr/0024-os-keyring-for-opds-credentials.md).

### Foundations

- [x] Typed IPC via tauri-specta —
      [ADR-0003](adr/0003-adopt-tauri-specta-v2-for-typed-ipc.md).
- [x] TanStack Query + hotkeys for client state and input —
      [ADR-0013](adr/0013-adopt-tanstack-query-and-hotkeys-for-client-state-and-input.md).
- [x] Secrets via sops+age —
      [ADR-0004](adr/0004-secrets-via-sops-age.md),
      [`doc/secret-management.md`](secret-management.md).
- [x] Filtered tracing with rolling-file output —
      [ADR-0002](adr/0002-adopt-filtered-tracing-with-rolling-file-output.md).
- [x] CI via mise tasks —
      [ADR-0006](adr/0006-continuous-integration-via-mise-tasks.md).
- [x] Architecture Decision Records —
      [ADR-0001](adr/0001-record-architecture-decisions.md).

## In progress

- [~] **Cover visual pipeline** — `blurhash` and `dominant_color`
      columns exist on `digital_inventory` and flow through
      `commands/catalog.rs` + `commands/import.rs`, but nothing derives
      them yet (both are always written as `None`). Remaining: generate
      a blurhash and a dominant colour when a cover is stored, so the
      library grid can render progressive placeholders.

## Backlog

- [ ] **Custom theming** — the settings page currently has Sync,
      Catalogs, and Keyboard tabs but no appearance controls. Add a
      user-facing light/dark/auto switch + accent colour; decide
      persistence location; layer WebAwesome tokens in the web app
      styles.

- [ ] **Directory scanner / file-watcher** — manual and batch file
      import are done ([ADR-0014](adr/0014-batch-file-import-with-progress-events.md)),
      but there is no watched-folder auto-ingest. Scope: directory
      scanner; file-watcher vs. manual trigger; dedup on move/rename;
      hydrates `digital_inventory` + `edition_files`.

- [ ] **Overdrive provider** — now a plugin rather than a Rust
      `Provider` impl. Open questions: library-auth flow (OAuth vs
      patron-barcode); modelling "available to borrow" status.

- [ ] **Plugin trust management GUI** — the host wires the stanchion
      registry (`commands/plugins.rs`) and can list signed plugins, but
      there is no user-facing UI to review signers, grant/revoke trust,
      or browse a discovery/repository client.

## How to read this

- Each Done item links the ADR that pinned the decision and/or the
  source path that implements it.
- Each Backlog item carries a sparse inline note describing scope and
  open questions — this ROADMAP is the source of truth.
- Repo-local task tracking lives in Taskwarrior (see
  [`doc/tasks.md`](tasks.md)); this ROADMAP and that task list are kept
  in sync by hand.
