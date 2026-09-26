# 25. Audiobook import and playback

Date: 2026-09-25

## Status

Accepted

## Context

The desktop imports EPUB and MOBI-family files natively but has no audiobook importer. iTunes MP4 (`.m4b`/`.m4a`) carries title, artist, narrator, date, genre, cover, duration, and chapters. The catalog already defines `KnownFormats::Audiobook`, `FormatMetadataSchema::Audiobook`, timestamp progress, and the narrator role, but no importer, per-edition storage, or playback path exists. ADR 0028's reader is accepted but unbuilt, so audio playback is the first reader integration.

## Decision

1. Add `crates/livtet-audio` on `mp4ameta` for `.m4b`/`.m4a`, returning `SourceMetadata` plus a validated `format_metadata` (`duration_seconds`, `chapters{name,audio_start,audio_end}`).
2. Add `Role::Narrator` (`nrt`), an optional `format_metadata` field on `SourceMetadata`/`ImporterMeta`, and route both extensions to `KnownFormats::Audiobook`.
3. Add core migration `m0011` with nullable `editions.format_metadata`, validated on write against the format's `FormatMetadataSchema`.
4. Play audiobooks in the dedicated `/reader/[editionId]` window, persisting `timestamp` position to `reading_progress`. Duration and chapters reach the UI through the typed `reader_publication` descriptor; `EditionDetail` stays BigInt-safe because specta forbids `serde_json::Value`.
5. Audio bytes are served over plain HTTP on 127.0.0.1 (ephemeral port, per-launch bearer token) via poem, not a custom `reader://` scheme: WebKitGTK routes `<audio>` through GStreamer, which cannot fetch custom schemes, so `reader://` URLs fail to load in the player. The frontend only sees the opaque `audio_url`.
6. The audio server fence is lexical on the library entry: the DB path must be absolute and stay under `books_dir` after `.`/`..` normalization, and the opened handle must be a regular file. Symlinks are never resolved — link-mode imports are symlinks inside `books_dir` pointing elsewhere by design, and resolving them 403s every linked file.
7. `editions.format_metadata` is deliberately absent from the `change_log` edition payload: the client triggers are `CREATE IF NOT EXISTS` and the payload shape is sync-wire, so syncing it needs its own client migration plus applier work (follow-up). Precedent: `group_id` is likewise stored but not synced.

## Consequences

### Becomes easier

- Audiobooks import natively with duration and chapters; playback and progress use the existing `Audiobook` schema and timestamp unit.
- The audio slice implements ADR 0028's window and streaming contract ahead of the EPUB navigator.

### Becomes harder or carries risk

- Requires a core-repo migration and cross-repo release before desktop persistence compiles.
- Linux WebKitGTK AAC/HE-AAC support must be verified; range serving and throttled progress writes are load-bearing.
