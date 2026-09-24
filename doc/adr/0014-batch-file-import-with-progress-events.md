# 14. Batch file import with progress events

Date: 2026-09-24

## Status

Accepted

## Context

The library has no way to add a book. `import_file` (ADR 0011) is implemented
but unwired, and importing a selection one call at a time gives the drawer no
progress and no single result to render. Selecting files needs a native picker,
and dropping files needs paths, not browser `File` objects.

## Decision

1. Extract the `import_file` body into `import_one` and add `import_files`, a
   sequential batch command returning one `ImportBatchResult` that also carries
   per-file `ImportFileResult`s. `import_file` is kept as the single-file
   primitive. The batch has no `Result` envelope: a per-file failure is data.
   Because Tauri rejects an async command that borrows state and returns a
   non-`Result`, the command takes only the `AppHandle` and fetches `AppState`
   from it.
2. Report progress over two Tauri events: `import://batch` (started/finished
   with the aggregate) and `import://file` (started/finished per path).
3. Adopt `tauri-plugin-dialog` for the native picker and Tauri's
   `onDragDropEvent` for dropped paths, both feeding the same
   `importFiles(paths)` frontend path.
4. Integer fields crossing IPC stay `i32` per the Specta width rule.

## Consequences

**Easier**: new import surfaces reuse one batch path and one reducer; the drawer
renders live per-file state and can retry only the failures.

**Harder**: import progress is now a second event family the frontend must keep
in sync with the command's return value; the plugin adds a JS and Rust
dependency and a capability entry.
