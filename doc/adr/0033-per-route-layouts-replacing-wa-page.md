# 33. Per-route layouts replacing wa-page

Date: 2026-10-04

## Status

Accepted.

## Context

`web/routes/+layout.svelte` wraps the application in one `<wa-page>` that owns
the header, navigation, body and footer for every route. Four problems follow.

Chrome cannot vary per route: library wants a toolbar over a scrolling grid,
settings wants tabbed panels, the splash wants a bare callout, yet all three go
through the same frame. The frame is also a web-page idiom — `mobile-breakpoint`
and a hamburger drawer — while the window minimum is 569×600, so "narrow" means
a small desktop window, not a phone. Height and scroll fight: `library/+page.svelte`
sets `main { height: 100% }` to reclaim height, then nests a `<wa-scroller>`
whose inner container never emits a bubbling `scrollend`, which is why
pagination there is driven by an `IntersectionObserver`. And styling the shell
means reaching through shadow DOM via `:global(wa-page[view='desktop'])`.

Two boundary leaks compound it: the shell reaches into a route by DOM id
(`getElementById('library-search')`) to implement `search.focus`, and the
library selection bar is `position: fixed` to the viewport, so it slides under
the navigation.

## Decision

Drop `<wa-page>`. Compose layouts from WebAwesome components, CSS grid and
WebAwesome tokens — no new layout dependency.

The shell (`+layout.svelte`) keeps only `AppFrame` — a `100dvh` grid with
`overflow: hidden` that never scrolls and hands the whole webview to the route —
plus `QueryClientProvider`, `Toaster`, `CommandPalette` and dev tools. It draws
no header, navigation, footer, or titlebar; the native window titlebar stands on
its own.

Five primitives live in `web/lib/layout/` and are placed by routes, not by the
shell: **`RouteShell`** (the route's nav/pane grid, which establishes the
container-query context), **`NavRail`** (vertical navigation a route may omit), **`Pane`** (vertical
stack, column flex, `position: relative` so docks anchor to it), **`ScrollRegion`** (a plain
`overflow: auto` element, exactly one per route), and **`Dock`** (pane-anchored
action bar). Height chains use `min-height: 0`; no `height: 100%`.

`data-scroll-region` marks **the element `Pane` grows**, which is not always the
element that scrolls. On `/settings` the attribute sits on the `wa-tab-group`
host so `Pane` bounds it, while the scrolling and the padding live on that
component's `::part(body)` inside its shadow root — which is how the layout
keeps the scroll contract instead of surrendering it to the component. A route
is still allowed exactly one scrolling element; because a generic
`querySelectorAll` cannot see into shadow roots, that rule is enforced by each
route's own tests rather than by one shared assertion.

Each route owns a layout beside it — `library/LibraryLayout.svelte`,
`settings/SettingsLayout.svelte`, and `web/lib/layout/ListLayout.svelte` shared
by `/catalog` and `/plugins` — carrying that route's title and actions in its
own toolbar. The
splash takes no navigation column.

Settings keeps `<wa-tab-group>`, but the layout owns the scroll contract through
`::part(body)`: adopting a WebAwesome component does not mean surrendering the
scroll region to it.

Responsiveness uses container queries on the route rather than viewport media
queries, so a layout reacts to its own width.

Because the web layer no longer reserves space for window controls,
`crates/livtet-desktop/tauri.conf.json` must revert `"titleBarStyle": "Overlay"`
and `"hiddenTitle": true` to a normally decorated window.

## Consequences

- Easier: a route changes its own chrome without touching the shell; scroll is
  one ordinary element per route, so `scroll` events bubble and the
  `<wa-scroller>` workaround retires; the selection dock anchors to its pane
  instead of the viewport; `search.focus` becomes a route-registered command
  rather than the shell reaching in by DOM id.
- Harder: navigation is composed per route, so a new route must place its own
  rail — the cost of letting routes decline it. Layout correctness now rests on
  `min-height: 0` discipline, which is easy to omit and silently breaks scroll.
- Risk: `e2e/app.spec.ts` asserts `wa-button[href="/settings"]` and a `Keyboard`
  heading. Keeping `NavRail` items as `<wa-button href=…>` preserves the former;
  the latter is expected to survive since tabs are retained, but must be
  confirmed, not assumed.
- `web/app.wa.js` drops `page/page.js` and gains `switch/switch.js`, which
  `/plugins` uses for a new per-plugin enable/disable toggle.

Scope: all eight routes. The five primary ones (`/`, `/library`, `/catalog`,
`/plugins`, `/settings`) plus `/catalog/[catalogId]` and the two reader routes,
`/reader/pub/[editionId]` and `/reader/audio/[editionId]`. The reader opens in a
second Tauri window via `open_reader`, but that window loads this same SPA, so
the reader is both a separate window and a route and inherits whatever the root
layout provides. The reader routes take `RouteShell` with no nav snippet —
application navigation is wrong in a reader window.
