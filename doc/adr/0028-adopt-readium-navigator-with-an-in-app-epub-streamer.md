# 28. Adopt Readium's navigator with an in-app EPUB streamer

Date: 2026-09-25

## Status

Accepted.

## Context

Livtet imports and catalogs EPUB files but cannot display them. Reading is a
first-class desktop feature: a book must open in its own window at
`/reader/pub/{editionId}` without leaving the library.

Readium (readium.org) is the reference stack for EPUB rendering, but it is
split into a **Streamer** (turn a packaged publication into a Readium Web
Publication Manifest, a Positions List, and individual resource bytes) and a
**Navigator** (render those resources). The TypeScript toolkit
(`@readium/shared`, `@readium/navigator`) is the Navigator; it ships no
EPUB/zip parser. In the reference deployments the Streamer is the Go toolkit
(`readium serve`), which streams a manifest and resources over HTTPS. A
desktop app that only embeds the TS Navigator therefore has nothing to render
unless it supplies its own Streamer.

The navigator also constrains *how* bytes must be served:

- `EpubNavigator` reads each spine XHTML through a `Fetcher`, re-serializes it
  into a `blob:` iframe, sets `<base href="{self-link base}">`, and injects its
  own CSP into that document. Images, stylesheets, and fonts are then loaded by
  the browser against that base.
- A `blob:` document inherits its creator's CSP, so both the injected CSP and
  the application CSP apply to those subresource loads.
- `fetch()` against a custom URI scheme is not reliably supported by the
  webview, so the navigator's programmatic reads cannot use the custom scheme.

`livtet-epub` already carries a hardened in-crate zip/OCF/OPF parser
([ADR 0018](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md)),
and library files live in `books_dir` behind `digital_inventory.file_path`
([ADR 0022](0022-library-owned-book-files-symlink-default-copy-opt-in.md)).

## Decision

Adopt the Readium TypeScript navigator for rendering and make the desktop app
its own Streamer.

### 1. New `livtet-reader` crate

A workspace member `crates/livtet-reader` owns publication streaming. It
depends only on `livtet-epub` plus `serde`/`serde_json`, holds no database or
Tauri dependency, and exposes:

- `Reader::open(path) -> Result<Reader>` — fail-closed on unreadable or
  unsupported input.
- `Reader::manifest(base_url) -> serde_json::Value` — Readium Web Publication
  Manifest (`@context`, `metadata`, `readingOrder`, `resources`, `toc`,
  `links`) whose `rel="self"` link is `{base_url}manifest.json`. The document
  `title` is mandatory for `Metadata.deserialize`, so it falls back to the
  filename.
- `Reader::positions() -> Vec<Location>` — Readium Locators.
- `Reader::read(href) -> Option<(String, Vec<u8>)>` — media type plus bytes for
  an OCF-relative href.

`livtet-epub` is widened to expose its existing parser: a public `Archive`
(`open`/`contains`/`read`/`entries`) and accessors for the OCF root, OPF path,
and raw OPF XML. Reads should be `&self` (the archive decodes from an
`Arc<Vec<u8>>`) so the desktop can serve concurrent requests from one shared
reader. `read_metadata` and the import pipeline are unchanged.

### 2. Two read paths

The navigator's programmatic reads (spine XHTML and any `Publication.get`) go
over IPC through a TypeScript `Fetcher` backed by
`reader_resource(editionId, href) -> String`. In v1 this returns UTF-8 text and
is used for spine XHTML only; Readium reads no other resource through the
fetcher.

Browser-resolved subresources (images, CSS, fonts, `<object>`) are served by a
custom `reader://` Tauri URI scheme registered with
`register_asynchronous_uri_scheme_protocol`. The manifest's base URL points at
that scheme, so relative hrefs resolve there. The handler validates the edition
and href, serves the archive entry with its manifest media type,
`Access-Control-Allow-Origin: *`, and `Cache-Control: no-store`, and answers
`400`/`404` on malformed or unknown paths.

Positions are passed directly to the `EpubNavigator` constructor, so no
Positions List link is emitted and `positionsFromManifest()` is never called.

### 3. Reader window

A Rust command creates a dedicated OS window labelled `reader-{editionId}` at
`WebviewUrl::App("reader/pub/{editionId}")` and focuses the existing window when one
is already open. Tauri's asset resolver falls back to `index.html`, so the SPA
route `/reader/pub/[editionId]` resolves on a cold load. The route parameter is the
edition `DbId` (a ULID), not a UUID. The publication base URL is
`reader://localhost/{editionId}/` on macOS/Linux and
`http://reader.localhost/{editionId}/` on Windows/Android; the
`reader_publication` command reports the platform-correct value, since Tauri v2
removed v1's `dangerousUseHttpScheme` and offers no way to force one form.

Creating the window from Rust avoids granting the frontend
`core:webview:allow-create-webview-window`.

### 4. Commands, state, and errors

| Command | Contract |
|---|---|
| `open_reader(editionId)` | Fail-closed on unknown edition, missing file, or non-EPUB format; builds or focuses the reader window. |
| `reader_publication(editionId)` | `{ baseUrl, manifest, positions }` for the navigator. |
| `reader_resource(editionId, href)` | UTF-8 text for the navigator's `Fetcher`. |

`AppState` gains a `readers` cache keyed by edition id, resolved from
`digital_inventory.file_path`; the entry is dropped when the reader window is
destroyed so deleted or re-linked files do not keep stale bytes. Errors are a
typed `ReaderError`; `livtet-reader` keeps its own crate-local error that the
desktop maps into it.

### 5. Capabilities and CSP

The default capability widens its `windows` to `["main", "reader-*"]` so reader
windows reach the commands. The application CSP is extended for the blob iframe
and the inherited subresource loads:

- `script-src 'self' 'unsafe-inline' blob:`
- `style-src 'self' 'unsafe-inline' blob: reader: http://reader.localhost`
- `img-src 'self' asset: data: https: blob: reader: http://reader.localhost`
- `font-src 'self' asset: data: blob: reader: http://reader.localhost`
- `media-src 'self' blob: reader: http://reader.localhost`
- `frame-src 'self' blob:` and `child-src 'self' blob:`
- `object-src blob: reader: http://reader.localhost` (relaxed from `'none'`
  because some EPUBs embed content with `<object>`)

`default-src 'self'`, `connect-src 'self'`, `frame-ancestors 'none'`, and
`base-uri 'self'` are unchanged: the fetcher uses IPC, and the blob document's
own CSP forbids fetches.

### 6. v1 scope

EPUB only; one position per spine item (chapter-granular progress); no table of
contents, reading settings, or position persistence. Non-EPUB formats get a
disabled Read action.

## Consequences

### Becomes easier

- One reader integration, with no Go sidecar and no loopback HTTP server to
  run, secure, and supervise.
- The streamer reuses the existing hardened zip/OCF/OPF parser rather than
  introducing another EPUB dependency.
- The routing requirement is satisfied directly: `/reader/pub/{editionId}` is a
  real SPA route in a real second window, shareable and reloadable.

### Becomes harder or carries risk

- CSP inheritance into `blob:` iframes is the fiddliest part; it must be
  verified on WebKitGTK, not only on WebView2.
- Positions are coarse, so progress is per-chapter until a splitter is added.
- `Reader` instances live in a cache; its lifetime is tied to the reader window
  so deleted or re-linked files do not keep stale bytes.
- The navigator runs in a separate window from the library, so nothing in the
  library observes reading state in v1.
- `reader_resource` is text-only in v1; a future navigator that reads binary
  resources through the fetcher would need a bytes variant.

### Amendment — media-specific reader routes

The generic `/reader/[editionId]` route is now namespaced by media kind:

- EPUB/Readium: `/reader/pub/[editionId]`
- Audiobook: `/reader/audio/[editionId]`

The shared `open_reader` command dispatches to the appropriate nested route.
The EPUB payload contract, custom `reader://` resource path, cache lifecycle,
and fail-closed validation above are unchanged. Audiobook playback remains on
its loopback server and preserves its existing descriptor contract.

### What future work can build on this

- Table of contents and the Readium preferences API
  (`EpubPreferencesEditor`) for font/theme/layout controls.
- Reading-position persistence in the client schema, seeded into the navigator
  as its initial `Locator`.
- A character-based positions algorithm to replace the coarse list.
- The decorator API for highlights/annotations, and non-EPUB navigators
  (audiobook, comic) once those formats are imported.

## Links

- [3. Adopt tauri-specta v2 for typed IPC](0003-adopt-tauri-specta-v2-for-typed-ipc.md)
- [18. Replace the `epub` crate with an in-crate OCF/OPF parser](0018-replace-the-epub-crate-with-an-in-crate-ocf-opf-parser.md)
- [22. Library-owned book files: symlink default, copy opt-in](0022-library-owned-book-files-symlink-default-copy-opt-in.md)
