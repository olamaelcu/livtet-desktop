# 49. Serve KOReader progress sync from the desktop

Date: 2026-10-31

## Status

Accepted

## Context

A book is read in more than one place: on this machine, and on an e-reader
running KOReader. KOReader already solves the e-reader half of that with its
"Progress sync" plugin — it pushes and pulls a document's position against a
small HTTP service. The official service is a remote account with Redis behind
it, and it knows nothing about this catalog: a position there is a `document`
digest and an opaque string, not an edition.

What the desktop already has is the other half: `reading_progress` (the current
position, per edition and format) and `reading_bookmarks`. Pairing a KOReader
device through `livtet-sync` does not help — that protocol moves the catalog
between Livtet clients, and a KOReader plugin will never speak it (ADR 0048
offers files to paired devices, not positions from a foreign client).

Alternatives considered:

1. **Point KOReader at the upstream sync service and reconcile later.** The
   account is remote, the payload carries no edition identity, and nothing would
   write `reading_progress`. Importing positions back would need credentials for
   a third-party service.
2. **Add the tables to `livtet-core`'s migration set.** That crate ships as a git
   dependency; a new local table cannot be added from here, which is exactly why
   reviews got their own table (ADR 0040).
3. **Add KOReader support to the sync daemon.** The daemon serves `/sync/*` with
   a pairing flow and session tokens. KOReader knows none of that, so the daemon
   would grow a second, unrelated protocol and a second settings file.
4. **Implement the plugin's protocol in the desktop over Poem.** The dependency
   is already in the tree (the OPDS server, ADR 0045), the lifetime is the app's,
   and the tables it needs are desktop-owned.

## Decision

Run a KOReader sync server in `koreader_sync.rs`, on the OPDS server's pattern:
a settings store (`koreader-sync.json`), a runtime Settings can start, stop, and
move (`koreader_sync_status | _settings_save | _start | _stop`), and a Poem route
table over the shared SQLite pool.

The server runs in a **child process** of the desktop binary, like
`livtet-sync-daemon`: `main` sees `--koreader-daemon` and enters a headless
Tokio runtime instead of Tauri. The desktop's `ProcessRunner` re-executes
`current_exe` with that flag, writes one `DaemonConfig` JSON line to its stdin
(settings, database path, books directory), and waits for a one-line readiness
report. The child binds the Poem listener, maps documents, and exits on stdin
EOF; the parent holds that pipe for the child's lifetime and kills it on stop or
replacement. A rebuild or crash is visible through `try_wait`, so `status`
reports not-listening until a save or start respawns it. Tests keep the
in-process runner, so the protocol and lifecycle tests do not need a second
binary.

**Protocol.** Serve the plugin's own paths, unabridged: `GET /healthcheck`,
`GET /users/auth`, `POST /users/create`, `PUT /syncs/progress`,
`GET /syncs/progress/:document`. The plugin appends these to the server URL the
reader types, so they cannot be nested under a namespace.

**Authentication.** Upstream uses `x-auth-user` and `x-auth-key` (the MD5 of the
password). Livtet has one local account, created in Settings: the username is
stored, only the MD5 of the password is stored, and registration over HTTP is
refused outright (`402`, as upstream does when registration is disabled). Every
route but `/healthcheck` requires the pair; an unpaired desktop refuses
everything, so the default configuration opens no useful surface. The server
binds every interface by default (`lan`), because the client is a tablet on the
network — the credentials, not the bind address, are the boundary.

The plaintext password is also written to a dedicated OS-keyring entry
(`net.olamaelcu.livtet.koreader`) so Settings can prefill the field on the next
launch. That copy is UX only: authentication still compares against the MD5 in
`koreader-sync.json`, and a missing or locked keyring degrades to an empty field
without blocking the save or the server.

**Documents.** KOReader names a book by a digest of its file: `util.partialMD5`
(1024-byte samples at `1024 << 2i` for `i` in `-1..=10`) or, with its "filename"
checksum method, the MD5 of the file name. `koreader_sync_ids` maps those digests
to `digital_inventory.id` and `edition_id`; it is filled by digesting the
library's own files — once at startup, bounded (`SWEEP_BATCH`) on a lookup that
misses — so a stock KOReader finds its books with no pairing dialog. The index on
`digital_inventory_id` is deliberately **not** unique: both checksum methods may
name the same file.

**Positions.** A push is written where the readers already read it: a PDF's page
and an audiobook's seconds go through `store_reader_position`, so opening the
book in Livtet resumes where KOReader stopped. A reflowable EPUB is the one
untranslatable direction — KOReader's position is an `xpointer`, Livtet's is a
Readium locator — so the raw string is kept per device in `koreader_sync_state`
and replayed to the other KOReader devices unchanged, while the percentage is
mirrored into `reading_progress.progress` for the `epub` unit (which no Livtet
writer reads). `koreader_sync_state.livtet_token` is a fingerprint of the row at
push time: a pull that finds Livtet's position moved since that push, and further
along, serves Livtet's instead, so reading in the app is not undone by an old
tablet.

**Notes and bookmarks.** The plugin syncs positions only, so `/koreader/status`,
`/koreader/books`, and `GET|POST /koreader/sync/book/{inventory_id}` are Livtet's
own extension: a push carries a position, notes merged into
`reading_progress.annotations` by `Annotation.id`, and bookmarks to add or delete
through the existing `reading_bookmarks` writers. Notes are merged, never
replaced: a device that knows one chapter must not erase the rest.

**Tables** (desktop-owned, `CREATE TABLE IF NOT EXISTS`, as with reviews in ADR
0040):

- `koreader_sync_ids` — `id` (the digest), `digital_inventory_id`, `edition_id`,
  `created_at`, `updated_at`; indexed on both ids.
- `koreader_sync_state` — `(device_id, edition_id)` primary key, `device_name`,
  `percentage`, `koreader_token` (the raw pushed position), `livtet_token`,
  `last_sync_at`.

## Consequences

An e-reader and this machine now share a reading position, and the position is
Livtet's own row rather than a foreign account's.

Easier: resuming a PDF or audiobook across devices is exact; a new book is
mapped by the next sync without user action; the plugin needs no fork.

Harder: an EPUB position written in Livtet cannot be handed to KOReader as an
`xpointer` — only the percentage crosses in that direction, so a Livtet-read
reflowable EPUB resumes in KOReader at the last position *KOReader* recorded
(Livtet still wins when it is further along). The percentage a KOReader push
leaves in `reading_progress.progress` is zeroed by the next Livtet position save,
because that writer owns the column for the `epub` unit; the per-device row keeps
it for pulls.

Risks: mapping digests reads every library file once — bounded per pass, but the
first pass on a large library spins the disk in the background. The credential is
one password with no attempt limit; the server is meant for a trusted LAN, and
turning it off remains the only hardening. `koreader_sync_ids` grows with the
library (two rows per file), which is acceptable for a local table but not free.
