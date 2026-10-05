# 37. Render PDFs with pdf.js in a dedicated reader window

Date: 2026-10-05

## Status

Accepted

## Context

PDFs import but cannot be read. [ADR 0027](0027-native-pdf-import-via-pdf-oxide-and-hayro.md)
added native `.pdf` extraction, so PDFs carry metadata, a cover, and a
`KnownFormats::Pdf` edition — yet every reader path refuses them. `open_reader`
dispatches audiobooks to `reader/audio/{id}` and sends *everything else* through
the EPUB resolver, whose `check_reader_file` hardcodes an `.epub` extension
test; `ReaderPublication` has only `Audiobook` and `Epub` variants; and
`canOpenInReader` in `web/lib/reader/read.ts` gates the library drawer's Read
button on epub. A PDF in the library is a dead end.

Rendering is the small part of the problem. The load-bearing question is how
PDF bytes reach the webview, and the CSP in `crates/livtet-desktop/tauri.conf.json`
constrains the answer: `connect-src 'self'` means neither `http://127.0.0.1:*`
nor `reader:` is fetchable, even though `media-src` already allows loopback for
`<audio>`. Scanned books — the ones users most want a PDF reader for — are
routinely 100–500 MB, so whole-file delivery is a real memory cost per open
window, paid once per reader window.

Three transports were considered:

1. **An IPC command returning raw bytes** (`tauri::ipc::Response`). No CSP,
   CORS, or server work, but the whole file stays resident in the webview.
2. **A route on the existing loopback server.** `commands/audio_server.rs`
   already has every hard part — edition resolution, the lexical
   `reader_path_in_library` fence, `parse_range_header`, `read_byte_range` —
   all tested. pdf.js consumes a URL and issues its own range requests, so
   memory stays flat regardless of file size.
3. **The `reader://` custom scheme.** It already sets
   `Access-Control-Allow-Origin`, but its handler is cache-gated to
   `livtet_reader::Reader` (EPUB-only) and custom schemes have no practical
   range support, so pdf.js would fall back to a whole-file fetch — option 1's
   memory profile with option 2's configuration cost.

## Decision

Render PDFs with pdf.js in a dedicated `reader/pdf/{editionId}` window, fed by
a ranged route on the loopback server (option 2).

### Transport

1. **The loopback server stops being about audio.** `commands/audio_server.rs`
   becomes `commands/byte_server.rs`; `AudioServer` → `LoopbackServer`,
   `AudioServerState` → `LoopbackServerState`, `AppState.audio` →
   `AppState.loopback`. The `/audio/:edition_id` route and its URL shape are
   unchanged.
2. **New route `GET /pdf/:edition_id?t=<token>`, plus `OPTIONS` for the
   preflight.** It reuses the per-launch bearer token check, the
   `reader_path_in_library` fence, `parse_range_header`, and `read_byte_range`
   unchanged. Only three things differ from `serve_audio`: `content-type:
   application/pdf`, the `KnownFormats::Pdf` gate, and CORS headers —
   `Access-Control-Allow-Origin: *`, `Allow-Headers: range`, `Expose-Headers:
   content-range, content-length, accept-ranges`, `Allow-Methods: GET, OPTIONS`.
   `OPTIONS` answers 204. A `Range` header is not CORS-safelisted, so pdf.js's
   ranged GET preflights; without the `OPTIONS` handler every range request
   fails.
   The wildcard origin is tolerable only because the token is 256 random bits
   generated per launch and never persisted — the same posture `/audio` already
   takes. Any local process can reach loopback; unguessable URLs are the
   defence, not the origin check.
3. **`connect-src` gains `http://127.0.0.1:*`** in the CSP. This is the only
   CSP change: the pdf.js worker is bundled same-origin by Vite, so it is
   already covered by `worker-src` falling back to `child-src 'self'`.
4. **`resolve_reader_audio` is factored, not copied.** It becomes
   `resolve_reader_file(db, edition_id, format)`, with `resolve_reader_audio`
   and a new `resolve_reader_pdf` as thin callers. The two differ only in their
   `KnownFormats` gate, and the `check-dupes` task would rightly flag a copy.

### IPC surface

5. **A third `ReaderPublication` variant:** `Pdf { edition_id, title, pdf_url }`.
   No page count — pdf.js reports `numPages` once the document loads, and
   carrying it would create a second source of truth that can disagree with the
   file.
6. **`reader_publication` dispatches Audiobook → Pdf → else Epub**, with
   `describe_pdf_publication` building the variant.
7. **`open_reader` grows a PDF branch.** `reader_pdf_window_path(&id)` yields
   `reader/pdf/{id}`; `open_pdf_reader` resolves the path and fence *before*
   building the window, so a missing or out-of-library file fails with no window
   ever appearing — the fail-closed order `open_epub_reader` already uses. It
   reuses the `READER_WINDOW_PREFIX{id}` label, so focusing an already-open
   window works unchanged, and unlike the EPUB branch there is no reader cache
   to evict when the build fails.
8. **`check_reader_file` takes the accepted extension as a parameter**, keeping
   its `EpubError::MissingFile` / `unsupported_format` mapping.
9. **`web/lib/reader/read.ts`** gains the `Pdf` payload type and
   `loadPdfReaderPublication`; `canOpenInReader` widens to accept pdf by format
   or extension, which is what lights up the drawer's Read button.
   `openEpubReader` already calls the format-agnostic `open_reader` and has one
   caller, so it becomes `openReader`. `web/lib/bindings.ts` is regenerated with
   `mise generate-bindings`.

### Route and rendering

10. **Scope is page-at-a-time parity with the EPUB reader**: title,
    Previous/Next/Close, one page in the viewport. No outline, no text layer, no
    saved position — the EPUB reader has none of these either.
11. **`+page.ts` follows the audio route**, not pub's bare `prerender = false`:
    the same ULID guard that 404s a garbage edition id, plus `prerender = false`
    for the static adapter.
12. **`+page.svelte` mirrors the pub route's skeleton** — `ReaderLayout`, header
    with `ActionButton` controls, `createLoadSession()` guarding overlapping
    loads, `phase` as `'loading' | 'ready' | 'error'`, and exactly one
    `[data-scroll-region]` (the contract `layout.browser.test.ts` enforces)
    wrapping a single `<canvas>`.
13. **`GlobalWorkerOptions.workerSrc` comes from
    `import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'`** so Vite
    emits the worker same-origin. `getDocument` sets `disableAutoFetch: true`;
    without it pdf.js eagerly pulls the whole file and the ranged transport buys
    nothing.
14. **`cMapUrl` and `standardFontDataUrl` point at `static/pdfjs/`**, populated
    from `pdfjs-dist/cmaps` and `pdfjs-dist/standard_fonts`. These load from
    `'self'`, already allowed. Omitting them does not error — it silently
    renders blank or garbled text on PDFs with CJK encodings or non-embedded
    standard fonts, which is exactly the scanned and non-English corpus this
    feature exists for.
15. **The in-flight `RenderTask` is cancelled before the next starts.** Rapid
    Next clicks otherwise interleave two draws onto one canvas. A
    `ResizeObserver` re-renders on resize; `onDestroy` cancels the task, calls
    `pdfDocument.destroy()`, and invalidates the session.
16. **`web/lib/reader/pdf.ts` holds the non-DOM logic** — page clamping to
    `[1, numPages]`, fit-to-width scale math including `devicePixelRatio`, and
    the cancellation handshake. Same rationale as `loadSession.ts` and
    `fetcher.ts` living outside the pub route: `+page.svelte` stays thin and the
    logic sits where the node test project can reach it.
17. **pdf.js's `PasswordException` surfaces as "this PDF is
    password-protected"** rather than a generic failure. Encrypted and DRM'd
    PDFs stay out of scope — [ADR 0036](0036-dedrm-binary-build-and-sidecar.md)
    owns that — but they fail legibly. The loopback token is never logged.

### Implementation plan

Test-first throughout; `mise test` and `mise lint` gate each step.

1. Rename the audio server to `byte_server.rs` and rethread `AppState.loopback`;
   existing audio tests must still pass untouched.
2. Factor `resolve_reader_file`, re-expressing `resolve_reader_audio` through it.
3. Add `resolve_reader_pdf` and the `/pdf` + `OPTIONS` routes with their tests.
4. Parameterize `check_reader_file`'s extension.
5. Add the `Pdf` variant, `describe_pdf_publication`, `reader_pdf_window_path`,
   `open_pdf_reader`, and the `reader_publication` / `open_reader` dispatch.
6. Widen `connect-src`; regenerate bindings.
7. Add `pdfjs-dist`, stage cmaps and standard fonts into `static/pdfjs/`.
8. Write `web/lib/reader/pdf.ts` against its unit tests; widen `read.ts` and
   rename `openEpubReader`.
9. Build the route, then the browser test asserting the single scroll region.

## Consequences

PDFs become readable, closing the gap ADR 0027 left, and the loopback server is
now the project's general answer to "a webview needs many bytes of a library
file with ranges" — a future CBZ or audio-adjacent reader inherits the fence,
range parsing, and token for free.

Costs and risks. The reader's IPC surface is now a three-arm union, so every
consumer of `ReaderPublication` must handle a third variant; the rename touches
`AppState` and the generated bindings, so it is a breaking change to both.
Widening `connect-src` to `http://127.0.0.1:*` lets any page in the webview
reach any loopback port, which is a real loosening justified by the token, not
by the origin. pdf.js is a large dependency with a separate worker bundle, and
its cmaps and standard fonts are build-time assets that can silently drift out
of sync with the pinned version — a version bump that moves those paths
degrades rendering without failing a test.

Not covered, deliberately: no e2e test, which needs a fixture PDF inside a real
library store; no outline, text layer, zoom, or remembered position; no
encrypted or DRM'd PDFs.

## Related

- [ADR 0027](0027-native-pdf-import-via-pdf-oxide-and-hayro.md) — native PDF
  import, which this completes.
- [ADR 0025](0025-audiobook-import-and-playback.md) — the loopback server this
  generalizes.
- [ADR 0033](0033-per-route-layouts-replacing-wa-page.md) — the per-route layout
  and single-scroll-region contract.
- [ADR 0036](0036-dedrm-binary-build-and-sidecar.md) — DRM removal, which owns
  encrypted PDFs.
