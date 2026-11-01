# 55. Tell the app when a pull changed the library

Date: 2026-11-01

## Status

Accepted

## Context

The library view is a TanStack Query cache, so it shows what some fetch
returned. Everything that writes the catalog in this app also drops that cache
(imports, enrichment, an accepted offer). The daemon's pull does not: it runs
on its own 60-second clock, applies a peer's rows directly, and reported back
only through `tracing`. A book that arrived over sync appeared after the reader
navigated away and back — or never, if they stayed on the page.

The events that existed did not cover it. `sync.completed` is emitted by the
HTTP middleware when *this* device serves a peer's request — that fills the
other device's library, not this one. `offer.received` announces an offer,
which is a question, not a book. Nothing said "rows landed here".

## Decision

Core (`livtet-sync-server`):

- New notification `sync.pulled` with `{ device_id, applied, conflicts }`,
  emitted by `pull_device` itself when a pull applied at least one row — so a
  pull the UI asked for reports too, and no caller has to remember to. A pull
  that applied nothing stays silent: it changed no catalog state, and a
  refetch per tick per peer would be pure traffic.

Desktop:

- `sync.pulled` → `sync://pulled`, beside the existing mapping table.

Web:

- `refreshLibraryOnPull(queryClient)` subscribes to `sync://pulled` and
  invalidates the `search` and `catalog` domains. The whole domain, not one
  key: the library view builds its query shapes from the active filters, and a
  pull can touch any edition, so enumerating keys would mean reproducing that
  view's internals.
- It is mounted in the root layout, next to the offer prompt, so a pull while
  the reader is in Settings still updates the catalog when they come back.
- `sync://completed` is deliberately not handled (see above).

## Consequences

### Easier

- A book that syncs in shows up on its own, on whichever route is open, within
  a pull tick.
- The distinction between "we pulled" and "we served a pull" is now explicit in
  the event names rather than inferred from a log line.

### Harder / to watch

- Invalidation is coarse: an accepted pull refetches the visible page, its
  covers, and any open detail record. That is one round trip per tick with
  changes, which the catalog can afford; a busier sync would want a narrower
  signal (the affected edition ids are not in the payload).
- A command that pulls and then invalidates on its own answer will refetch
  twice (once for the command, once for the event); TanStack collapses the
  concurrent refetches of an active query, so this is a deduped no-op today.
- The desktop depends on a daemon revision that emits `sync.pulled`; an older
  sidecar simply never fires the event, so the library goes back to updating on
  navigation.
