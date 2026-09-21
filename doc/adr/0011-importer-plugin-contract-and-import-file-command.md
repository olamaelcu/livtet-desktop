# 11. Importer plugin contract and import_file command

Date: 2026-09-21

## Status

Accepted

## Context

`import_epub` calls `livtet-epub` directly, so every additional format would
require another command-specific parser wired into the desktop process.
Stanchion's out-of-process host isolates untrusted Lua, but it serves
`Registry<DynClass>`: it has no seam for registering a native importer and
checks methods dynamically at call time. `RemoteRegistry` is also not `Send`,
so it cannot live in Tauri `AppState`. Lua's table-to-JSON boundary is lossy:
empty tables arrive as `{}` rather than `[]`.

## Decision

1. **Add a typed `livtet-importer` contract.** It defines `#[lua_class]
Importer` with `extensions()` and `read_metadata()`, alongside the serde
`ImporterMeta` JSON boundary. The built-in `EpubImporter` implements that
contract natively by wrapping `livtet-epub`.
2. **Run importers through a dedicated fail-fast host.**
`crates/livtet-plugin-host` reuses Stanchion's `build_registry` and `serve`,
then refuses to start when any loaded plugin lacks an `ImporterHandle`
required method. `--contract report` instead strips violating plugins'
granted capabilities and serves introspection (`list`) only; `list_plugins`
uses report mode while imports use the default reject mode. This replaces the
stock `plugin-host` sidecar.
3. **Keep EPUB native and route other extensions to Lua.** `import_file`
selects `EpubImporter` for `.epub` and launches the host per non-EPUB call.
The host answers only `fs_read` for the exact selected file; covers travel as
base64 in `ImporterMeta`. Empty Lua tables are normalized to the contract's
known list fields before deserialization.
4. **Generalize the import command.** `import_epub` becomes `import_file`.
It preserves title/creator/ISBN fail-closed checks, hash dedup, the single
transaction, post-commit cover handling, and search indexing, while adding
`importer` and `unsupported` error codes and extension-mapped format IDs.
ISBNs are canonicalized through `Isbn::parse` before duplicate checks,
persistence, and API output. Remote imports hash the exact bytes served
through `fs_read`, and remote payloads are bounded (selected file, text,
list, and cover sizes) under code `importer`.

## Consequences

**Easier**: new formats arrive as data-directory Lua plugins implementing two
methods, without touching the desktop process. The contract is load-checked by
the host and content-checked by the command. EPUB keeps a direct,
high-performance native path.

**Harder**: every host plugin must presently satisfy the importer contract;
a malformed plugin fails the whole host at startup. Remote file and cover
payloads are base64 over JSON, adding roughly one-third overhead. This ADR
partially supersedes ADR 0009's direct `livtet-epub`/`import_epub` design.
