# Readium reader integration — design

Date: 2026-09-25
Status: Draft for implementation
ADR: [0028](../adr/0028-adopt-readium-navigator-with-an-in-app-epub-streamer.md)

## Goal

Open a book from the library in its own OS window whose webview navigates to
`/reader/{editionId}`, and render the EPUB there with the Readium TypeScript
navigator. The desktop app supplies the Readium **Streamer** (manifest,
positions, resource bytes); `@readium/navigator` supplies the **Navigator**.

`{editionId}` is the edition's `DbId` (a ULID), not a UUID. The path is
`/reader/{editionId}`.

## Background: what the navigator requires

The design is driven by three facts about `EpubNavigator` (verified against
`readium/ts-toolkit` `develop`):

1. **Programmatic reads go through a `Fetcher`.** `EpubNavigator` calls
   `publication.get(link)` and `FrameBlobBuilder` does
   `await this.pub.get(item).readAsString()` for every spine XHTML document.
   `Publication` is constructed as
   `new Publication({ manifest, fetcher })` from a Readium Web Publication
   Manifest (RWPM) and a `Fetcher`. Positions come from
   `Publication.positionsFromManifest()` **or** are passed directly to the
   `EpubNavigator` constructor. `load()` requires a non-empty positions list.

2. **The document is re-serialized into a `blob:` iframe.** FrameBlobBuilder
   parses the XHTML, injects Readium CSS and the injectables loader, inserts
   `<base href="{item.toURL(publication.baseURL)}">` and its own
   `<meta http-equiv="Content-Security-Policy">`, then returns
   `URL.createObjectURL(new Blob([...]))`. `FrameManager` then creates
   `<iframe sandbox="allow-same-origin allow-scripts">` and navigates it to
   that blob URL. Images, CSS, and fonts in the EPUB are loaded by the browser
   against the `<base>` href — **not** through the `Fetcher`.

3. **`baseURL` is derived from the manifest's `self` link.**
   `Manifest.baseURL` strips the last path segment of the `rel="self"` href, so
   a self link of `reader://localhost/{id}/manifest.json` yields the base
   `reader://localhost/{id}/`.

Consequences:

- Readium's programmatic reads must not depend on the custom scheme, because
  `fetch()` to a custom scheme is unreliable in the webview. They use IPC.
- Subresources must be reachable at `<base> + relative-href`, so the scheme
  must serve every archive entry by path.
- A `blob:` document inherits the creator's CSP, and CSPs intersect. The
  application CSP must therefore allow `blob:` frames and the subresource
  scheme, not just the entry document.

## Tauri facts (v2.11.6)

- `register_asynchronous_uri_scheme_protocol("reader", handler)` serves
  `reader://localhost/<path>` on macOS/Linux and
  `http://reader.localhost/<path>` on Windows/Android. Tauri v2 has **no**
  `dangerousUseHttpScheme` (that was v1, Windows-only); `use_https_scheme` on
  `WebviewBuilder` is about the app's own origin, not custom protocols. The
  base URL must therefore be computed per platform.
- The built-in asset resolver falls back to `index.html` when a path is not a
  bundled file, so a window opened at `reader/{editionId}` cold-loads the SPA
  and SvelteKit resolves `/reader/[editionId]`.
- Only one `invoke_handler` can be registered, and every command registered
  through `tauri_specta::Builder` must be `specta::Type`-safe. Commands needing
  raw returns are avoided (see `reader_resource` below).

## Architecture

```
library window (main)                     reader window (reader-{editionId})
┌───────────────────────────┐             ┌──────────────────────────────────────┐
│ BookCard dblclick         │             │ /reader/[editionId]                  │
│ EditionDetailDrawer Read  │  invoke     │  ReaderFetcher ── IPC ──┐            │
│        │                  │ ──────────▶ │  EpubNavigator          │            │
│        └─ open_reader ────┼─ Rust ───▶  │    │ blob iframe       │            │
└───────────────────────────┘  builds     │    │ <base href>       │            │
                               window     │    └─ img/css/font ────┼─ reader:// │
                                          └───────────────────────────┬──────────┘
                                                                      │
                          Rust: AppState.readers[editionId] ── livtet-reader::Reader
```

## Component 1 — `livtet-reader` crate

New workspace member `crates/livtet-reader`. Dependencies: `livtet-epub`,
`serde`, `serde_json`. No database, no Tauri, no async. It reads one local
EPUB file.

```rust
pub struct Reader { /* open Archive + parsed OCF/OPF */ }

impl Reader {
    /// Open and parse an EPUB. Fail-closed on unreadable/unsupported input.
    pub fn open(path: &Path) -> Result<Self, ReaderError>;

    /// RWPM manifest (`@context`, `metadata`, `readingOrder`, `resources`,
    /// `toc`, `links`) with the `self` link set to `{base_url}manifest.json`.
    pub fn manifest(&self, base_url: &str) -> serde_json::Value;

    /// Readium Locators. v1: one per reading-order item, progression 0.
    pub fn positions(&self) -> Vec<Location>;

    /// Resolve an OCF-relative href to (media type, bytes).
    pub fn read(&self, href: &str) -> Option<(String, Vec<u8>)>;

    /// True when the href names an entry that exists in the archive.
    pub fn contains(&self, href: &str) -> bool;
}

#[derive(Serialize)]
pub struct Location {
    pub href: String,
    #[serde(rename = "type")]
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub locations: LocationDetail,
}

#[derive(Serialize)]
pub struct LocationDetail {
    pub progression: f64,
    pub position: u32,
    #[serde(rename = "totalProgression")]
    pub total_progression: f64,
}
```

`livtet-reader` defines its own crate-local `ReaderError` (open/parse/read
failures); the desktop maps it into `ReaderError::Publication` for IPC.

Manifest shape (minimum that `Manifest.deserialize` accepts):

```json
{
  "@context": ["https://readium.org/webpub-manifest/context.jsonld"],
  "metadata": {
    "title": "…",
    "author": ["…"],
    "language": ["en"],
    "readingProgression": "ltr",
    "conformsTo": ["https://readium.org/webpub-manifest/profiles/epub"]
  },
  "readingOrder": [{ "href": "OEBPS/ch01.xhtml", "type": "application/xhtml+xml" }],
  "resources": [{ "href": "OEBPS/style.css", "type": "text/css" }],
  "toc": [{ "href": "OEBPS/ch01.xhtml", "title": "…" }],
  "links": [{ "rel": ["self"], "href": "reader://localhost/{id}/manifest.json" }]
}
```

Notes:

- `title` is mandatory for `Metadata.deserialize`; the reader falls back to
  the filename when the OPF has none.
- A Positions List link is deliberately **not** emitted: positions are passed
  to the `EpubNavigator` constructor, so `positionsFromManifest()` is never
  called. This keeps a single positions source.
- `mediaType` falls back from the OPF manifest to an extension map
  (`.xhtml` → `application/xhtml+xml`, `.css` → `text/css`, etc.).
- Entries outside the OCF root are not addressable; href resolution is
  directory-relative to the OPF and normalized (the crate already rejects
  path traversal and backslashes).

### `livtet-epub` widening

`livtet-reader` reuses the existing parser rather than introducing a second
zip implementation. Minimal public surface added to `livtet-epub`:

- `pub mod archive` (re-export of the current `zip::Archive`): `open(bytes)`,
  `contains(&str)`, `read(&str)`, and a new `entries() -> Vec<String>`.
- A parsed container/package accessor (OCF root + OPF path + raw OPF XML),
  built from the existing `ocf`/`xml` modules.
- Prefer making `Archive::read`/`decode` `&self` (they already decode from an
  `Arc<Vec<u8>>`), so the desktop cache can hand out `Arc<Reader>` without a
  mutex on the hot path. If that is not possible, wrap each reader in a
  `Mutex`.

`read_metadata` and the import pipeline are untouched.

## Component 2 — `livtet-desktop` backend

### State

```rust
// types.rs
pub struct AppState {
    // …existing fields…
    /// Open publications, keyed by edition id. Entries are dropped when the
    /// corresponding reader window closes.
    pub readers: Arc<Mutex<HashMap<DbId, Arc<Reader>>>>,
}
```

Lookup path: `editions` → `digital_inventory.file_path` (already exposed by
`get_edition_detail`). A missing path or a missing target is a `ReaderError`.

### Custom protocol

Registered in `run()` before `.setup`:

```rust
tauri::Builder::default()
    .register_asynchronous_uri_scheme_protocol("reader", |ctx, request, responder| {
        // reader://localhost/{edition_id}/{href}   (macOS/Linux)
        // http://reader.localhost/{edition_id}/{href} (Windows)
    })
```

Handler behaviour:

1. Strip host and leading slash; the first path segment is the edition id, the
   remainder is the archive href.
2. Resolve the edition's file path; open the `Reader` if not cached.
3. `reader.read(href)` → respond `200` with the entry bytes, the manifest's
   media type as `Content-Type`, `Access-Control-Allow-Origin: *`, and
   `Cache-Control: no-store` (v1).
4. Unknown edition/href → `404`; malformed path → `400`.

The handler is async (`UriSchemeResponder`) and must not block the event loop on
disk I/O it cannot avoid; the archive is already read into memory by
`Archive::open`, so entry reads are in-memory.

### Commands (`commands/reader.rs`)

All registered in `collect_commands!` and `ReaderError` derives `specta::Type`.

| Command | Signature | Notes |
|---|---|---|
| `open_reader` | `(editionId: String) -> Result<(), ReaderError>` | Fail-closed on unknown edition, missing file, or non-EPUB format. Focus `reader-{id}` if present, else build it. |
| `reader_publication` | `(editionId: String) -> Result<ReaderPublication, ReaderError>` | `{ baseUrl, manifest: serde_json::Value, positions: Vec<Location> }`. `baseUrl` is platform-correct. |
| `reader_resource` | `(editionId: String, href: String) -> Result<String, ReaderError>` | UTF-8 text for the navigator's `Fetcher`. v1 reads spine XHTML only; binary subresources go through the scheme. |

`open_reader` constructs the window in Rust:

```rust
WebviewWindowBuilder::new(app, format!("reader-{id}"), WebviewUrl::App(
    format!("reader/{id}").into(),
))
.title(edition_title)
.inner_size(1000.0, 720.0)
.min_inner_size(480.0, 480.0)
.build()?;
```

Building in Rust avoids granting the frontend
`core:webview:allow-create-webview-window`.

### Errors (`error.rs`)

Matches the house style in `src/error.rs` (`#[derive(Debug, Clone, Error, Serialize, Type)]`,
named-field variants, helper constructors):

```rust
#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum ReaderError {
    #[error("No edition with id: {id}")]
    UnknownEdition { id: String },

    #[error("This edition has no file on disk")]
    NoFile,

    #[error("This edition's file is missing; re-link it first")]
    MissingFile,

    #[error("The {format} format is not readable yet")]
    UnsupportedFormat { format: String },

    #[error("Could not read the publication: {message}")]
    Publication { message: String },
}
```

## Component 3 — capabilities and CSP

`capabilities/default.json`:

```json
"windows": ["main", "reader-*"]
```

so reader windows (same origin, same trust boundary) reach the three commands.
No new permissions are required for the commands themselves; the reader window
needs `core:window:allow-close` (already granted) to close itself.

`tauri.conf.json` → `app.security.csp`:

```
default-src 'self';
script-src 'self' 'unsafe-inline' blob:;
style-src 'self' 'unsafe-inline' blob: reader: http://reader.localhost;
img-src 'self' asset: data: https: blob: reader: http://reader.localhost;
font-src 'self' asset: data: blob: reader: http://reader.localhost;
media-src 'self' blob: reader: http://reader.localhost;
connect-src 'self';
frame-src 'self' blob:;
child-src 'self' blob:;
frame-ancestors 'none';
object-src blob: reader: http://reader.localhost;
base-uri 'self'
```

Rationale:

- `blob:` in `script-src`/`frame-src`/`child-src` — the navigator's iframe and
  the injectables loader live on blob URLs.
- Both scheme forms in the subresource directives so one CSP works on every
  platform; the non-matching form is inert.
- `connect-src 'self'` stays: the `Fetcher` uses IPC, and the iframe's own CSP
  (`connect-src 'none'`) forbids fetches inside the blob document anyway.
- `object-src` is relaxed from `'none'` because some EPUBs embed content with
  `<object>`; only `blob:` and the reader scheme are allowed.

## Component 4 — frontend

### Dependencies

```
@readium/shared
@readium/navigator
@readium/navigator-html-injectables
@readium/css
```

### `web/lib/reader/fetcher.ts`

```ts
export class ReaderFetcher implements Fetcher {
  constructor(private readonly editionId: string) {}
  links(): Link[] { return [] }
  get(link: Link): Resource { return new ReaderResource(this.editionId, link) }
  close() {}
}
```

`ReaderResource`:

- `read()` → `new TextEncoder().encode(await invoke('reader_resource', { editionId, href }))`.
- `readAsString()` → the raw string (avoids a redundant UTF-8 round-trip).
- `link()` → the original `Link`.
- `close()` → no-op.
- A small `Map<string, string>` cache keyed by href so a spine item fetched
  twice is read once per reader window.

### `web/lib/reader/open.ts`

```ts
export function openReader(editionId: string) {
  return invoke('open_reader', { editionId })
}
```

### `web/routes/reader/[editionId]/+page.svelte`

Lifecycle:

1. `page.params.editionId` → `reader_publication`.
2. `Publication` from `{ manifest: Manifest.deserialize(manifest),
   fetcher: new ReaderFetcher(editionId) }`.
3. `new EpubNavigator(container, publication, listeners, positions, undefined,
   { preferences: {}, defaults: {} })`, then `await navigator.load()`.
4. Listeners: `positionChanged` (v1: update a local progression indicator),
   `frameLoaded`, `handleLocator` (open external links via the opener plugin).
5. Chrome: book title, previous/next buttons delegating to
   `navigator.goBackward` / `goForward`, and a close button calling
   `getCurrentWebviewWindow().close()`.
6. `onDestroy` → `navigator.destroy()`.
7. States: loading, unsupported format, missing file (with a re-link hint), and
   a generic read error.

Keyboard paging is handled by the navigator's injectables; no custom hotkey
wiring in v1.

### Entry points

- `web/lib/components/EditionDetailDrawer.svelte`: a **Read** `ActionButton` in
  the File section, enabled only when `file.file_format` is `epub` and
  `file.file_status !== 'missing'`; otherwise disabled with a reason.
- `web/routes/library/BookCard.svelte`: accept `ondblclick`; the library page
  wires double-click to `openReader(editionId)` (double-click stays separate
  from selection, which uses single click).

### Bindings

`mise run generate-bindings` regenerates `web/lib/bindings.ts`; `pnpm check`
must pass with the new commands.

## Data and control flow

1. User double-clicks a card / presses Read → `invoke('open_reader')`.
2. Rust validates the edition + file, focuses or builds `reader-{id}` at
   `reader/{id}`.
3. The reader webview loads the SPA, resolves `/reader/[editionId]`.
4. The page calls `reader_publication` → manifest + positions + base URL.
5. The page constructs the `Publication` and `EpubNavigator`, calls `load()`.
6. For each visible spine item, FrameBlobBuilder asks the fetcher for the
   XHTML → IPC → `Reader::read`. The resulting blob iframe resolves images,
   CSS, and fonts against the base URL → `reader://` protocol → `Reader`.
7. Window close drops the `AppState.readers` entry (see Risks).

## Testing and verification

- `cargo test -p livtet-reader`
  - manifest has a `self` link, non-empty `readingOrder`, a title, and a
    position per reading-order item;
  - `read()` returns the expected bytes and media type for a spine item, CSS,
    and an image;
  - traversal hrefs (`../…`), absolute paths, and unknown hrefs are rejected;
  - an EPUB with no navigable spine fails `open` with `Publication`.
  Uses `livtet-epub`'s existing test-support fixtures.
- `cargo test -p livtet-desktop`: `ReaderError` mapping; `open_reader` on an
  unknown edition; `reader_publication` on an edition with no file.
- `pnpm test`: `ReaderFetcher` encodes/decodes and caches; a route smoke test
  with `invoke` mocked.
- `mise lint` and `mise test`.
- Manual/automated verification with the Tauri MCP bridge: open a seeded
  edition, assert the reader window renders text and paging moves the
  progression. This is the only check that exercises real CSP + blob iframes.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Parent→blob CSP inheritance differs per webview engine | Verify on WebKitGTK first (primary platform); keep both scheme forms in the app CSP. |
| `Metadata.deserialize` needs a `title` | Fall back to the filename in `livtet-reader::manifest`. |
| Coarse positions make progress chapter-granular | Acceptable for v1; documented as a follow-up. |
| Stale `Reader` cache after delete/re-link | Drop the cache entry on reader-window close (`WindowEvent::Destroyed`), and validate file existence in `open_reader`. |
| Encrypted/DRM and remote-resource EPUBs | Out of scope; surface a `Publication` error rather than rendering partial content. |
| `reader_resource` returns text only | v1 spine XHTML is text; binary subresources bypass IPC via the scheme. |

## Out of scope (v1)

- Table of contents UI, reading settings (font/theme/layout), progress
  persistence.
- Non-EPUB formats (MOBI/AZW3/PDF): the Read action is disabled.
- Annotations/highlights, search within a book, audiobook/comic navigators.

## File manifest

New:

- `crates/livtet-reader/Cargo.toml`
- `crates/livtet-reader/src/lib.rs`
- `crates/livtet-reader/src/manifest.rs`
- `crates/livtet-reader/src/positions.rs`
- `crates/livtet-reader/src/error.rs`
- `crates/livtet-reader/tests/stream.rs`
- `crates/livtet-desktop/src/commands/reader.rs`
- `web/lib/reader/fetcher.ts`
- `web/lib/reader/open.ts`
- `web/lib/reader/fetcher.test.ts`
- `web/routes/reader/[editionId]/+page.svelte`

Changed:

- `Cargo.toml` (workspace member + `livtet-reader` dependency)
- `crates/livtet-epub/src/lib.rs`, `zip.rs`, `ocf.rs`, `xml.rs` (public surface)
- `crates/livtet-desktop/Cargo.toml`, `src/lib.rs`, `src/types.rs`,
  `src/error.rs`, `src/commands/mod.rs`
- `crates/livtet-desktop/capabilities/default.json`
- `crates/livtet-desktop/tauri.conf.json`
- `web/lib/components/EditionDetailDrawer.svelte`
- `web/lib/library/BookCard.svelte`, `web/routes/library/+page.svelte`
- `package.json`, `pnpm-lock.yaml`
- `web/lib/bindings.ts` (generated)

Note: the working tree already contains uncommitted changes to
`crates/livtet-epub/src/lib.rs`, `metadata.rs`, `extraction.rs`,
`EditionDetailDrawer.svelte`, and `+page.svelte`; coordinate with those before
editing.
