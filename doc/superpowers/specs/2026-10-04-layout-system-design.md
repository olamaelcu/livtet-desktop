# Layout system without `<wa-page>`

Date: 2026-10-04
Status: approved, pending implementation plan

## Problem

`web/routes/+layout.svelte:104` wraps the whole application in a single
`<wa-page>`. That one element owns the header, the navigation, the page body and
the footer for every route, and four consequences follow.

**Chrome cannot vary per route.** Every route is pushed through one frame.
Library wants a toolbar over a scrolling grid, settings wants tabbed panels,
and the splash wants a bare centred callout. Today all three negotiate with the
same header/nav/footer whether they want it or not.

**It is a web page, not a desktop window.** `mobile-breakpoint="960"` and the
hamburger drawer at `+layout.svelte:106` are browser idioms. The window's
minimum is 569×600 (`crates/livtet-desktop/tauri.conf.json`), so "narrow" means
a small desktop window, not a phone.

**Height and scroll fight.** `library/+page.svelte:443` sets `main { height: 100% }`
to claw back the height `<wa-page>` controls, then nests a `<wa-scroller>` whose
inner container never emits a bubbling `scrollend` — forcing the
`IntersectionObserver` workaround documented at `library/+page.svelte:233-237`.

**Styling means reaching through shadow DOM.** `+layout.svelte:170` targets
`:global(wa-page[view='desktop'])` to hide a control the shell itself rendered.

Two boundary leaks compound this. The shell reaches into a route by DOM id at
`+layout.svelte:40` (`document.getElementById('library-search')`), and the
library's selection bar is `position: fixed` to the viewport
(`library/+page.svelte:539`), so it slides under the navigation.

## Decision

Replace `<wa-page>` with a thin shell plus layouts that live beside their
routes, composed from a small set of primitives. No new layout dependency:
WebAwesome components plus CSS grid and WebAwesome design tokens.

### The shell

`web/routes/+layout.svelte` keeps only what is global:

- `AppFrame` — a `100dvh` grid with `overflow: hidden` that never scrolls and
  hands the entire webview to the route.
- `QueryClientProvider`, `Toaster`, `CommandPalette`, and the dev-only
  `SvelteQueryDevtools`.

It renders no header, no navigation, no footer, and **no titlebar**: the native
window titlebar stands on its own and the web layer draws nothing for it.

### Primitives — `web/lib/layout/`

Routes place these; the shell places none of them.

| Primitive | Responsibility |
| --- | --- |
| `RouteShell` | The route's nav/pane grid: one column, or `auto 1fr` when a nav is supplied. Establishes the container-query context (`container-type: inline-size`). |
| `NavRail` | Vertical navigation. Icon-only by default, expandable. A route may omit it. |
| `Pane` | Vertical stack (column flex) whose final child is the scroller. `position: relative`, so docks anchor to the pane. |
| `ScrollRegion` | A plain `overflow: auto` element. **Exactly one per route.** |
| `Dock` | Pane-anchored floating action bar. |

A `Pane` grows its `ScrollRegion` specifically (it targets `[data-scroll-region]`,
not source order), so a `Dock` may sit before or after the region.

`ScrollRegion` being an ordinary div is the point: `scroll` events bubble and it
can serve as an `IntersectionObserver` root, which retires the `<wa-scroller>`
workaround. Every height chain uses `min-height: 0` on grid children; no
`height: 100%` anywhere.

### Route layouts

Each layout sits beside the route it serves and carries that route's title and
actions in its own toolbar, because the shell no longer has anywhere to put them.

- `web/routes/library/LibraryLayout.svelte` — rail, toolbar, filter strip, one
  grid `ScrollRegion`, and a `Dock` for selection mode.
- `web/routes/settings/SettingsLayout.svelte` — rail, toolbar, and a
  `<wa-tab-group>`.
- `web/lib/layout/ListLayout.svelte` — shared by `/catalog` and `/plugins`:
  rail, toolbar, one list `ScrollRegion`. It sits with the primitives rather
  than beside a route because two routes share it.
- `/` (splash) — no navigation column at all. It redirects after 1.5s, so a rail
  would only flash.

Settings keeps `<wa-tab-group>`, but **the layout owns the scroll contract**:

```css
.settings__tabs::part(body) { flex: 1 1 auto; min-height: 0; overflow: auto; }
```

Adopting a WebAwesome component does not mean surrendering the scroll region to
it. This also removes the need for a width rule here, because `wa-tab-group`'s
nav already scrolls horizontally when narrow.

### Responsiveness

Container queries on the route, not viewport media queries, so a layout reacts
to its own width. One rule collapses the rail below `46rem` because the rail is
a primitive. `mobile-breakpoint` and the hamburger drawer are removed.

### Leaks closed

- `search.focus` becomes a route-registered command handler instead of the shell
  calling `getElementById('library-search')`.
- The `:global(wa-page[view='desktop'])` rule disappears with the element.

## Required changes outside the web layer

`crates/livtet-desktop/tauri.conf.json` sets `"titleBarStyle": "Overlay"` with
`"hiddenTitle": true`. On macOS that makes webview content run *under* the
titlebar. Since the web layer will no longer reserve space there, this must
revert to a normally decorated window, or the library toolbar renders beneath
the traffic lights.

`web/app.wa.js` drops the `page/page.js` import and gains `switch/switch.js`
(used by the plugin enable/disable toggle and the sync settings).

## Scope

In scope, all eight routes:

- Primary: `/`, `/library`, `/catalog`, `/plugins`, `/settings`.
- Secondary: `/catalog/[catalogId]`, `/reader/pub/[editionId]`,
  `/reader/audio/[editionId]`.

An earlier draft of this spec claimed the reader was "a separate Tauri window,
not a SvelteKit route" and scoped the work to five routes. That was wrong.
`open_reader` (`web/lib/reader/read.ts:27`) opens a second Tauri *window*, but
that window loads the same SPA at `/reader/pub/[editionId]` — so the reader is
both a separate window and a route, and it inherits whatever the root layout
provides. Removing `<wa-page>` therefore affects it.

The three secondary routes need different treatment from the primary five:

- `/catalog/[catalogId]` is a browsing list and takes the full chrome —
  navigation rail, toolbar, one scroll region. It currently contains no `href`
  links at all, so without a rail there is no way to leave it.
- The two reader routes open in their own window, where application navigation
  is wrong. They take `RouteShell` with no nav snippet and run full-bleed.
  `/reader/pub/[editionId]` must also lose its `height: 100%`, which violates
  the height-chain constraint.

Also in scope, because the prototype surfaced it: `/plugins` rows gain a
`<wa-switch>` that enables and disables a plugin, reflected in the row's status
badge.

## Testing

Unit and component tests keep the existing hooks `.card-wrap`, `#library-search`,
`.result-count`, and `.suggestion-item`, so library specs are unaffected.

Known e2e impact in `e2e/app.spec.ts`:

- line 20 asserts `wa-button[href="/settings"]`. Keeping `NavRail` items as
  `<wa-button href=…>` preserves it and keeps real link semantics.
- line 22 asserts a `Keyboard` heading. With `<wa-tab-group>` retained this
  likely survives; confirm during implementation rather than assuming.

New coverage to add:

- Exactly one scroll region per route, and the frame itself never scrolls.
- The settings tab body scrolls while its pane does not.
- The plugin toggle changes plugin state and its badge.
- At 569px width the rail is icon-only.

Verification commands: `mise test` and `mise lint`.

## Prototype

A throwaway static prototype demonstrating all of the above lives in
`prototype/` (untracked): `layout.html`, `wa-entry.js`, `vite.config.mjs`, and
`shoot.mjs`, served over `python3 -m http.server 4321`. It is not proposed for
the application and should not be committed.

Measured on the prototype, with no console errors: zero `<wa-page>` instances,
no web-drawn titlebar, the frame does not scroll, library has exactly one scroll
region, the settings tab body scrolls while the pane does not, the plugin toggle
moves a row from `enabled` to `disabled`, and the rail is 52px at 569px width.

## Alternatives rejected

**Shared primitives with no shell.** Maximum isolation, but the navigation rail
would be duplicated across five routes and drift.

**A single `<AppLayout>` component with header/sidebar/body snippets.** This is a
mechanism rather than an alternative, and used alone it recreates the original
problem: one frame's opinions imposed on every route. Snippets are still the
right technique *inside* each per-route layout.

**Master/detail for settings.** Prototyped and rejected: it changes settings UX
beyond the layout work this spec covers.
