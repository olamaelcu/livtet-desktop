# Glossary

Single source for livtet-desktop terms. One entry per concept. See the `glossary` skill for lookup/add/audit workflow.

Naming policy: use the canonical term from code or ADRs. Expand abbreviations on first use. Keep definitions short.

## UI

_No entries yet._

## IPC

- **import_file** — the Tauri command that imports a book through the importer selected for its extension. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).

## Backend

- **fs_read** — host-to-application capability callback that returns the selected import file's bytes, scoped to that exact path. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **plugin host** — the `livtet-plugin-host` sidecar that runs Lua importers out of process. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).

## Domain

- **Importer** — a file-format plugin implementing the `livtet-importer` contract (`extensions`, `read_metadata`); native EPUB in-app or remote Lua in the plugin host. See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
- **ImporterMeta** — the serde wire record an importer returns (title, contributors, ISBNs, cover). See [ADR 0011](adr/0011-importer-plugin-contract-and-import-file-command.md).
