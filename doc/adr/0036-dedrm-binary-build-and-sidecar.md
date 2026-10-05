# 36. deDRM post-processor binary: compile from the noDRM source and sidecar into Tauri

Date: 2026-10-05

## Status

Draft — pending user approval and the build-mise task implementation.

## Context

The import pipeline needs a DRM post-processor that runs **before** format parsing and dedup hashing, so that a DRM-encrypted Kindle AZW3 and the same book from a non-DRM source are not deduped against each other.

The post-processor is exposed to the Rust plugin host as a **Lua-only** plugin: it is discovered from `plugin.toml`, loaded through the existing stanchion Lua runtime, and its entrypoint is a Lua script that calls external tools via `os.execute`. The Lua script is not the decryption logic itself.

The deDRM functionality is delivered as the **noDRM DeDRM tools** (the actively maintained fork of the Calibre plugin). Its decryption logic — key extraction (`kindlekeys.txt`, Kindle local-storage keys, Adobe ADE ID + `.adept`, Kobo SQLite) and AES-CTR DEK decryption of PalmDOC text records — is pure Python. The only runnable front-ends are the Calibre plugin (requires Calibre) or the planned standalone CLI (`DeDRM_plugin/standalone/remove_drm.py`), which is explicitly marked *"not functional yet"*.

The user has confirmed:

- No Python runtime may be assumed at call time, so `os.execute` must target a compiled **binary**, not a Python script.
- The Lua plugin must not declare a `python` backend — only `lua` is permitted.
- The binary must be built in the build pipeline and shipped with the app.

## Decision

1. **Source**: clone the noDRM DeDRM tools at build time and compile the CLI front-end to a native binary with **Nuitka** (`--standalone --onefile`). Nuitka is used because the source is Python and a true executable must be produced; `pip install` at runtime is rejected since no Python runtime can be assumed.

2. **Build task**: add `.mise/tasks/build-dedrm-binary` (file-based bash task), triggered by the `ci` task (`mise ci`), which:
   - installs Nuitka (`pip install nuitka`) in an isolated env,
   - clones `noDRM/DeDRM_tools` into `.build/dedrm` (or `target/dedrm`),
   - compiles `DeDRM_plugin/standalone/remove_drm.py` to `dedrm` via Nuitka,
   - copies the resulting ELF to `crates/livtet-desktop/binaries/dedrm-x86_64-unknown-linux-gnu`.

3. **Platform coverage (first pass)**: ship binaries for **Linux** (`x86_64-unknown-linux-gnu`) and **macOS** (`x86_64-apple-darwin` for Intel; `aarch64-apple-darwin` for Apple Silicon) as the `externalBin` entry in `tauri.conf.json`. Windows is out of scope for this ADR.

4. **Tauri sidecar**: register `binaries/dedrm-<triple>` in `bundle.externalBin` alongside `livtet-sync-daemon` and `livtet-plugin-host`, so Tauri resolves the sidecar path at runtime and Tauri exposes it to the renderer via `src-tauri/binaries/`.

5. **Lua plugin usage**: the Lua entrypoint (`plugins/dedrm_loader.lua`) receives the input path from the host, calls `os.execute("binaries/dedrm-x86_64-unknown-linux-gnu --infile <path> --outfile <outdir>")`, and returns the decrypted output path. If no DRM is detected (or the run fails), the host treats the file as a **passthrough** and uses the original bytes unchanged, so dedup and the normal format importers continue to work exactly as before.

## Consequences

If adopted: the post-processor becomes self-contained — no Calibre, no system Python, no per-user `pip install`; every user runs the same vetted binary with the same key-extraction logic the plugin author ships.

If rejected or deferred: the Lua plugin would have to fall back to calling an external `dedrm` executable that the user installs and maintains themselves, which breaks the "no extra install" expectation and puts versioning/supersetting of key extraction in the user's hands.

## Related

- ADR 0033 (per-route layouts)
- ADR 0011 (importer plugin contract)
- ADR 0012 (sidecar daemons via tauri `externalBin`)
- Follow-up: `.mise/tasks/build-dedrm-binary`, `crates/livtet-importer/src/post_processor.rs`, `crates/livtet-desktop/src/commands/import.rs`
