# Covers

How book covers are fetched, stored, encoded, and served in livtet-desktop.

## Architecture

Two traits from `livtet-covers` define the pipeline:

- **`CoverFetcher`** — resolves an edition to candidate cover URLs
  (by querying the DB for identifiers), then downloads raw image bytes
  from a remote provider.
- **`CoverStorage`** — stores cached bytes in a hash-keyed file store,
  promotes cache entries to permanent files, and lists covers for a
  given inventory item.

The pipeline: **fetch → store → encode → write metadata**.

## Provider chain

Three providers implement `CoverFetcher`, searched in priority order:

### Google Books (priority 0 — tried first)

`keys_for` queries `work_identifiers` for entries where `kind = "google_books"`,
parses the volume ID from `urn:google_books:{id}`, and builds a `CacheKey`.

`fetch` constructs a Google Books thumbnail URL:
```
https://books.google.com/books/content?id={volume_id}&printsec=frontcover&img=1&zoom={zoom}
```

### Hardcover (priority 1)

`keys_for` queries `work_identifiers` for entries where `kind = "hardcover"`, parses
the numeric ID, then makes a GraphQL call to `api.hardcover.app/v1/graphql` to
resolve the work to its image URL.

`fetch` performs a direct GET on the resolved image URL.

### OpenLibrary (priority 2 — tried last)

`keys_for` queries `edition_identifiers` → `identifiers` for ISBNs (kind = `"isbn"`),
strips the `urn:isbn:` prefix, and builds `CacheKey`s for sizes S, M, L.

`fetch` constructs the OpenLibrary Covers API URL:
```
https://covers.openlibrary.org/b/isbn/{isbn}-{size}.jpg
```

## Storage

`CacacheStorage` (in `tauri/src/cover_storage.rs`) implements `CoverStorage`
using a hash-keyed filesystem layout:

```
{app_cache_dir}/covers/
  entries/
    {SipHash(key)}          ← raw image bytes, keyed by hash of the content key
  markers/
    {inventory_id}/
      {SipHash(key)}        ← empty file recording the association
{app_local_data_dir}/covers/
  {inventory_id}/
    cover.{ext}             ← permanent file (written by copy_to_permanent)
```

### Content key format

```
{provider}::{identifier_type}::{identifier_value}::{size}::{ext}
```

Example: `openlibrary::isbn::9780141439518::M::jpg`

### Marker keys

Markers are stored in `markers/{inventory_id}/{SipHash(content_key)}`.
The marker file contains the content key string, not the image bytes.
`list_cached()` walks the markers directory for a given `inventory_id`,
reads the content keys, reads the entry bytes, and returns `CachedCover`
structs.

## Encoding

Once a cover is stored at its permanent path, `encode_cover()` (from
`livtet-covers`) computes:

- **`blurhash`** — a 4×3 pixel string for progressive loading placeholders
- **`dominant_color`** — the average sRGB hex color of the full image

These are written to `digital_inventory.blurhash` and
`digital_inventory.dominant_color`. The `cover_path` column stores the
permanent file path.

## Frontend

### Current state

- **Remote search results**: `CoverCard` renders `<img>` tags using the
  `cover_url` from the provider's search response (OpenLibrary CDN,
  Google Books CDN, Hardcover CDN).
- **Local catalog results**: `cover_url` is always `None`. `CoverCard`
  falls back to a letter-on-color placeholder using a hash-derived
  background color from `cover-art.ts`.
- **Edition detail (Files tab)**: `file-row.svelte` shows a color swatch
  from `digital_inventory.dominant_color` when available.

### Future

When a `serve_cover` command or asset protocol handler is wired, local
catalog results will show actual cover images loaded from the permanent
directory. `blurhash` can be rendered as an inline SVG placeholder while
the full image loads.

## Commands (planned)

| Command | Purpose |
|---|---|
| `fetch_cover(edition_id)` | Chain `CoverFetcher` providers by priority, download bytes, pass to `CoverStorage`, encode, write `blurhash`/`dominant_color` to `digital_inventory`. |
| `list_covers(inventory_id)` | Return `Vec<CachedCover>` for display in the edition detail Files tab. |
| `serve_cover(path)` | Tauri asset protocol handler to stream cached cover images. |

## Related

- ADR-0008: Hash-keyed file cover storage with provider-backed fetchers
- `core/livtet-covers/src/` — trait definitions and `encode_cover()`
- `tauri/src/cover_storage.rs` — `CacacheStorage` implementation
- `tauri/src/commands/remote_search/google_books.rs` — `CoverFetcher` impl
- `tauri/src/commands/remote_search/hardcover.rs` — `CoverFetcher` impl
- `tauri/src/commands/remote_search/openlibrary.rs` — `CoverFetcher` impl
