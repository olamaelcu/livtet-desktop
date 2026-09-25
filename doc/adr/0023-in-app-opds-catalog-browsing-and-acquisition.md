# 23. In-app OPDS catalog browsing and acquisition

Date: 2026-09-25

## Status

Accepted

## Context

Livtet can import local EPUB/PDF files but cannot pull titles from OPDS
catalogs (Gutenberg, Standard Ebooks, Internet Archive, or a user's own
catalog). A sibling workspace, `livtet-opds` (`github.com/olamaelcu/livtet-opds`),
already ships `livtet-opds-client` — feed parsing (OPDS 1.x Atom + 2.0 JSON),
search-template expansion, and pagination — but the desktop app has no
integration and no way to store catalog subscriptions or credentials.

Constraints: the Tauri CSP keeps `connect-src 'self'`, so remote fetches must
run in Rust; `specta-typescript` refuses to export the upstream wire types
because they carry `u64` fields (BigInt precision); and credentials must not
leak across origins or to the frontend.

## Decision

1. Depend on `livtet-opds-client` (with its `atom` feature for OPDS 1.x) and
   `livtet-opds-types` from the `livtet-opds` git `main`, and add a shared
   `reqwest::Client` plus a plugin-store handle to `AppState`.
2. Persist catalog subscriptions in a Rust-owned `tauri-plugin-store` file
   (`opds-catalogs.json`). Credentials live in that file; list responses expose
   only the auth kind. CRUD probes the feed before writing (fail-closed).
3. Keep every remote fetch in Rust (`opds_feed`, `opds_page`, `opds_search`,
   `opds_acquire`, `opds_catalogs_test`) so the CSP is untouched. Pagination is
   stateless: the frontend passes the next `href` back.
4. Return flattened, specta-safe DTOs (`OpdsFeed`, `OpdsPublication`,
   `OpdsNavigation`) instead of upstream wire structs, side-stepping the
   `u64`/BigInt export restriction and moving link selection into Rust.
5. `opds_acquire` downloads through the shared client into a temp file (2xx
   required before writing) and reuses the existing import pipeline rather than
   `DownloadManager`, which has no resume/integrity checking and reports 404
   bodies as complete.
6. Security: only `http`/`https` URLs; credentials refused over plain `http`
   off-loopback and attached only to same-origin requests; page links restricted
   to the catalog origin.

## Consequences

**Easier**: subscribed catalogs and their credentials are managed in one place;
acquisition reuses dedup, cover extraction, and indexing; the DTO boundary is
the seam for richer OPDS features (facets, groups) without re-exporting upstream
types.

**Harder**: desktop now carries `reqwest`/`livtet-opds-*`; the DTO mapping must
be kept in sync with any upstream model changes. Credentials originally lived
inline in the catalog store; [ADR 0024](0024-os-keyring-for-opds-credentials.md)
moves them to the OS keyring.
