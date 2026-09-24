# 13. Adopt TanStack Query and Hotkeys for client state and input

Date: 2026-09-23

## Status

Accepted

## Context

Frontend state was hand-rolled. `web/routes/library/+page.svelte` tracked
books, pagination offsets, a loading flag, and a debounced typeahead by hand.
`web/lib/components/SyncPanel.svelte` polled six commands on a 5 s
`setInterval`, re-implemented a refresh cache with a `busy` flag, and
re-fetched after every sync event. `@tanstack/svelte-hotkeys` was already a
dependency but unused, so keyboard interactions (Escape, Enter) were bespoke
DOM handlers.

The app is a client-rendered SPA (`ssr = false` in `web/routes/+layout.ts`)
that talks to the Rust backend over Tauri IPC. The "server" is local and
cheap, but the request/response lifecycle is still asynchronous server state.

## Decision

Adopt three TanStack libraries on the frontend:

* **Query** (`@tanstack/svelte-query`, v6, runes) owns all client server-state
  over IPC. One `QueryClient` (`web/lib/query/client.ts`) is provided at the
  root layout, keys live in a single factory (`web/lib/query/keys.ts`), and
  queries and mutations are declared per feature.
* **Hotkeys** (`@tanstack/svelte-hotkeys`) owns the keyboard layer: a command
  registry (`web/lib/hotkeys/commands.ts`), global bindings in the root
  layout, a `Mod+K` command palette, and user-customizable bindings recorded
  through the Hotkey Recording API and persisted with
  `@tauri-apps/plugin-store` (`web/lib/hotkeys/bindings.svelte.ts`).
* **Query Devtools** (`@tanstack/svelte-query-devtools`) is mounted in the
  root layout behind `import.meta.env.DEV`.

Sync data is event-authoritative: `sync://*` events invalidate the affected
queries and a 30 s `refetchInterval` remains only as a fallback, down from
polling as the primary mechanism.

The unified `@tanstack/svelte-devtools` shell is intentionally not adopted:
it ships no Svelte Query plugin, so it would display no query state.

Alternatives rejected: hand-rolled runes and caches (the duplication this
change removes), TanStack Router (SvelteKit already routes), and TanStack
Table/Form (the library is a card grid and the forms are trivial).

## Consequences

Easier: cache ownership, request deduplication, background refresh, and
mutation-driven invalidation become library concerns, and the Sync panel and
Library page shrink to declarative queries. Keyboard shortcuts, display
formatting, and rebinding come from one registry instead of bespoke handlers.

Harder: the frontend now depends on TanStack's Svelte adapters (Query v6,
Hotkeys 0.11), which track Svelte 5 runes closely. Query defaults are tuned
for local IPC (`refetchOnWindowFocus: false`, short `staleTime`, bounded
retry) rather than the library's network-oriented defaults, and should be
reviewed when new query families are added. Devtools remain dev-only.
