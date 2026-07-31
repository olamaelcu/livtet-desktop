# 8. Hash-keyed file cover storage with provider-backed fetchers

Date: 2026-07-31

## Status

Accepted

## Context

`livtet-desktop` needs to fetch and store cover images so local catalog
search results and edition detail views can display real covers instead of
letter-on-color placeholders. The `livtet-covers` crate (`core/livtet-covers`)
defines two trait-based abstractions:

- `CoverFetcher` — given an edition ID and DB connection, produces `CacheKey`
  structs with the identifiers needed to download a cover, then fetches the
  raw image bytes from a remote provider.
- `CoverStorage` — stores fetched bytes under a cache key, promotes cached
  entries to permanent filesystem paths associated with an inventory ID, and
  lists covers cached for a given inventory.

Neither trait had a concrete implementation in the desktop crate. The
`digital_inventory` table already carries the three cover-metadata columns
(`cover_path`, `blurhash`, `dominant_color`) but no code populates them.
Local catalog search results always show `cover_url: None`.

Three remote search providers — Google Books, Hardcover, and OpenLibrary —
already implement the `Provider` trait (search). They each own an HTTP
client and know how to construct cover URLs from their API responses, making
them natural candidates to also implement `CoverFetcher`.

The original plan was to use `cacache` (npm's content-addressable cache
ported to Rust) as the `CoverStorage` backend. However, `cacache` v13.1.0
does not compile on Rust 1.97 — the `tokio` feature flag exposes conflicting
re-imports and duplicate trait impls. Turning off default features uncovered
further missing types (reliance on `async_std` internals even in sync mode).

## Decision

### 1. File-based cover storage

Drop `cacache` in favor of a simple hash-keyed filesystem store
(`CacacheStorage` in `tauri/src/cover_storage.rs`). Two directory trees
sit under the app's cache and data directories:

| Directory | Purpose |
|---|---|
| `{app_cache_dir}/covers/entries/` | Content-addressed byte storage — filename is `SipHash(key)` |
| `{app_cache_dir}/covers/markers/` | Per-inventory index — `{inventory_id}/{SipHash(key)}` files containing the content key string |
| `{app_local_data_dir}/covers/` | Permanent files — `{inventory_id}/cover.{ext}` |

This is content-addressable in the same spirit as cacache — entries are
keyed by a hash of their logical key, preventing filesystem-naming issues
with special characters in cache keys — but with zero external dependencies
beyond std and `fs_err`.

The `CacacheStorage` struct implements `livtet_core::covers::CoverStorage`
and is managed as `Arc<CacacheStorage>` on Tauri's `AppState`.

**Cache key encoding convention** (Approach B from the plan):
```
{provider}::{identifier_type}::{identifier_value}::{size}::{ext}
```

`store()` writes bytes into the entries directory. `copy_to_permanent()`
reads the entry, writes it to the permanent path, and creates a marker
file recording the association. `list_cached()` walks the marker directory
for the given inventory ID, parses content keys, reads entries, and returns
`CachedCover` structs.

### 2. Provider-backed `CoverFetcher` implementations

Each existing provider struct gains a second trait impl:

| Provider | Priority | `keys_for` DB query | `fetch` URL |
|---|---|---|---|
| Google Books | 0 (first) | `work_identifiers` where kind = `"google_books"`, parse `urn:google_books:{id}` | `https://books.google.com/books/content?id={id}&printsec=frontcover&img=1&zoom=1` |
| Hardcover | 1 | `work_identifiers` where kind = `"hardcover"`, GraphQL resolve to image URL | direct GET on resolved URL |
| OpenLibrary | 2 (last) | `edition_identifiers` → `identifiers` where kind = `"isbn"`, strip `urn:isbn:` prefix | `https://covers.openlibrary.org/b/isbn/{isbn}-M.jpg` |

Priority ordering favours Google Books (fastest API, indexed cover serving),
then Hardcover (authed, higher-quality covers), then OpenLibrary (public,
no auth, robust ISBN lookup).

Hardcover's `keys_for` makes a secondary GraphQL call
(`books(where: {id: {_eq: …}})`) to resolve the stored work ID to an image
URL. This is the only `keys_for` that performs network I/O. If the
Hardcover API key is unavailable or the call fails, `keys_for` returns an
empty `Vec<CacheKey>`, allowing the chain to fall through to OpenLibrary.

### 3. No new Tauri commands

This is infrastructure only. The `CoverStorage` and `CoverFetcher`
instances are consumed by future commands (a `fetch_cover` command, or
integration into the `import_edition` flow).

## Consequences

### Becomes easier

- Any edition with stored identifiers can have its cover fetched and
  cached through a single trait dispatch across the provider chain.
- Adding a new provider (e.g. Amazon, OCLC) is two impl blocks — one for
  search (`Provider`), one for covers (`CoverFetcher`).
- The hash-keyed file store is self-contained: no external dependencies,
  no schema, no lock contention. Entries and markers are plain files.

### Becomes harder or carries risk

- `DefaultHasher` (SipHash-1-3) is not collision-resistant. Two different
  cache keys that hash to the same value would silently overwrite each
  other. This is acceptable because:
  - Cache keys are internally generated, not user-controlled.
  - A collision would only cause a stale cover to be shown — it would
    not corrupt data or expose anything.
  - If this ever matters, swapping `DefaultHasher` for SHA-256 is a
    one-line change (`hash_key` function).
- Hardcover's GraphQL round-trip in `keys_for` adds ~200ms latency to
  cover resolution. Acceptable because cover fetching is paid once per
  edition at import time, not inline in search results.
- `list_cached` reads all bytes eagerly. For editions with many covers
  (multiple sizes from multiple providers), this could spike memory.
  Mitigation: cover images are typically < 200KB each; the method is
  called in a backend command, not in a hot rendering loop.
- The marker file approach relies on the filesystem for listing. For
  thousands of covers, a single `read_dir` call is fine; if this becomes a
  bottleneck, switching to a SQLite-backed index is a natural migration
  path.

### What future work can build on this

- A `fetch_cover` Tauri command that chains `CoverFetcher` providers by
  priority, passes results to `CoverStorage`, and calls `encode_cover()`
  to populate `blurhash`/`dominant_color` on the `digital_inventory` row.
- Integration into `import_edition` so covers are fetched at import time.
- A `serve_cover` Tauri asset protocol handler that streams from the
  permanent directory.
- Replace `DefaultHasher` with blake3 or SHA-256 if content-addressable
  semantics are ever needed (e.g. deduplication across installations).
- Store the content-type (`image/jpeg`, `image/webp`) in the marker file
  alongside the content key for accurate `Content-Type` headers.

## Links

* Covered by `doc/covers.md`
* [7. Normalize language codes with isolang during import](0007-normalize-language-codes-with-isolang-during-import.md)
