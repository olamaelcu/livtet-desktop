# 51. Opt-in Spotlight indexing with delegate activation

Date: 2026-10-31

## Status

Accepted

## Context

Livtet's library is only searchable inside the app. macOS users expect app
content to appear in Spotlight, and this library is a natural fit: each edition
has a title, authors, description, tags, subjects, identifiers, and often a
cover.

CoreSpotlight provides the index API, but three constraints shape the design:

- Apple only accepts index writes from a signed app. Release DMGs are Developer
  ID signed and notarized; a `tauri dev` bare binary is not, so development
  validation is unreliable.
- Selecting a result delivers `application:continueUserActivity:restorationHandler:`
  with `CSSearchableItemActionType`. `tao` implements that selector but returns
  `false` for every activity except `NSUserActivityTypeBrowsingWeb`
  (`app_delegate.rs`, tao 0.37.1), so Tauri's `RunEvent::Opened` never sees it.
- Index writes need no entitlement on this app (it is not sandboxed), but the
  index is visible system-wide, so indexing must be a deliberate choice.

## Decision

Add an opt-in Spotlight integration, macOS only.

- **Settings**: a `spotlight.json` store holds `{ enabled, indexed_at, item_count }`.
  A new macOS-only **Desktop** settings tab owns the switch and a rebuild
  button. The tab is filtered with the same `navigator.platform` check the
  window chrome uses (`web/lib/platform.ts`); the panel is not rendered on
  other platforms at all.
- **Indexing**: `crates/livtet-desktop/src/spotlight.rs` reads every edition
  with its authors, description, tags, subjects, identifiers, and cover path in
  bulk, and `spotlight/macos.rs` writes one `CSSearchableItem` per edition to
  the default index under the domain `net.olamaelcu.livtet.library`. Unique ids
  are `livtet:edition:<id>`. Each rebuild deletes the domain first, so editions
  deleted while the app was closed disappear. Covers become a 128 px PNG in
  `thumbnailData`; unreadable covers are skipped rather than failing the write.
- **When**: enabling writes immediately; disabling deletes the domain; startup
  refreshes the index in the background when the setting is on; the panel
  exposes **Rebuild index**. Import, batch import, virtual-book creation,
  deletion, tag add/remove, enrichment, and Calibre import call
  `schedule_reindex`, which coalesces into one full rebuild five seconds after
  the first mutation. A full rebuild keeps deletion handling trivial; it is
  cheap enough for a personal library and only runs after a write.
- **Activation**: `install_activation` swizzles the existing
  `NSApplicationDelegate`'s `application:continueUserActivity:restorationHandler:`
  implementation at setup. It claims only `CSSearchableItemActionType`, reads
  `kCSSearchableItemActivityIdentifier`, and forwards every other activity to
  the original IMP. The claimed id reveals the main window and emits
  `spotlight://open-edition`; the root layout routes to
  `/library?edition=<id>` and the library page opens the detail drawer, then
  removes the parameter.
- **No new IPC contract for selection**: the event is web-side only; the
  existing `spotlight_status`, `spotlight_set_enabled`, and `spotlight_reindex`
  commands are Specta-typed and return `{ code, message }` errors.

## Consequences

- Easier: Spotlight exposes the whole library with rich results; choosing one
  opens the exact edition; opting out removes every trace in one domain delete.
- Harder: the app now depends on an unsafe, in-process AppKit swizzle that must
  forward unknown activities correctly. A future tao change that starts
  handling searchable-item activities would let this hook be removed.
- Risk: unsigned development builds may index nothing, so the feature must be
  validated from a signed `.app` bundle. The integration is coverage-excluded
  with the rest of the native chrome.
- Privacy: indexing is off by default. Once on, titles and authors are visible
  to Spotlight; disabling deletes the domain but does not clear macOS logs or
  other system caches.
