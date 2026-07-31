# Project Roadmap

The goals of this project are as follows:

- provide a local library experience
- allow for custom theming of the library interface
- local file management of books
- cover management

## Status legend

`[x]` done · `[~]` in progress · `[ ]` backlog

## Implemented

### Search & metadata

- [x] OpenLibrary provider —
      `tauri/src/commands/remote_search/openlibrary.rs`,
      [ADR-0003](adr/0003-adopt-tauri-specta-v2-for-typed-ipc.md).
- [x] Google Books provider (app-wide API key) —
      `tauri/src/commands/remote_search/google_books.rs`,
      [ADR-0003](adr/0003-adopt-tauri-specta-v2-for-typed-ipc.md),
      [ADR-0004](adr/0004-secrets-via-sops-age.md).
- [x] Hardcover provider (per-user OS keychain) —
      `tauri/src/commands/remote_search/hardcover.rs`,
      `tauri/src/commands/keyring.rs`,
      [ADR-0003](adr/0003-adopt-tauri-specta-v2-for-typed-ipc.md).
- [x] Edition detail view (peek dialog + `/catalog/[editionId]` route) —
      `web/lib/catalog/`,
      [#2](https://github.com/olamaelcu/livtet-desktop/pull/2),
      [ADR-0005](adr/0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md).

### Data model

- [x] `digital_inventory` 1:1 with editions (UNIQUE) —
      [ADR-0005](adr/0005-pin-digital-inventory-edition-id-to-unique-add-edition-detail-ipc-surface.md),
      [livtet#1](https://github.com/olamaelcu/livtet/pull/1).
- [x] `edition_files` 1:N (plugin-scanned) —
      [livtet#1](https://github.com/olamaelcu/livtet/pull/1) (m0010).

### Foundations

- [x] Typed IPC via tauri-specta —
      [ADR-0003](adr/0003-adopt-tauri-specta-v2-for-typed-ipc.md).
- [x] Secrets via sops+age —
      [ADR-0004](adr/0004-secrets-via-sops-age.md),
      [`doc/secret-management.md`](secret-management.md).
- [x] Filtered tracing with rolling-file output —
      [ADR-0002](adr/0002-adopt-filtered-tracing-with-rolling-file-output.md).
- [x] Taskwarrior integration —
      [`doc/tasks.md`](tasks.md).

## In progress

(none)

## Backlog

- [ ] **Overdrive provider** (flagged by library selection) —
      library-auth flow; OAuth vs patron-barcode auth model; how to
      model "available to borrow" status; 4th `Provider` impl in
      `tauri/src/commands/remote_search/`.

- [ ] **Custom theming** —
      user-facing light/dark/auto switch + accent colour; persistence
      location (tauri-store vs settings.json); WebAwesome token
      layering in `web/app.css`.

- [ ] **Local file ingestion** —
      directory scanner; file-watcher vs manual import; dedup on
      move/rename; hydrates `digital_inventory` + `edition_files`;
      cross-repo impact on `core/livet-data`.

- [ ] **File management UI** —
      `/library` route; grid vs list; sort/filter; actions (open in
      OS reader, reveal in Finder, delete from library); how it
      relates to the OPDS catalog.

- [ ] **Cover pipeline** —
      fetch `cover_url` from search hits to a local cache; integrate
      with `digital_inventory.cover_path` + `blurhash` /
      `dominant_color`; format choice (webp?); offline behaviour.

## How to read this

- Each Done item links the source path + the ADR that pinned the
  decision.
- Each Backlog item carries a sparse inline note describing scope
  and open questions — the ROADMAP is the source of truth.
- Implementation of a Backlog item begins by promoting its inline
  note into a spec issue + Taskwarrior milestones.