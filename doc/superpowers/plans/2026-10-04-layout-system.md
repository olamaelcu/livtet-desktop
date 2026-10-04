# Layout System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single `<wa-page>` shell with a thin `AppFrame` plus layouts that live beside their routes.

**Architecture:** The root layout keeps only a full-height non-scrolling frame and the global overlays. Five primitives in `web/lib/layout/` are placed *by routes*, not by the shell, so a route can decline navigation or vary its chrome. Each route owns exactly one scroll region.

**Tech Stack:** SvelteKit 2 (SSR off, static adapter), Svelte 5 runes and snippets, WebAwesome 3.10 web components, Vitest (`unit` + `browser` projects), Playwright for e2e, Tauri 2.

**Spec:** [doc/superpowers/specs/2026-10-04-layout-system-design.md](../specs/2026-10-04-layout-system-design.md)

## Global Constraints

- No new layout dependency. WebAwesome components, CSS grid, and WebAwesome design tokens only.
- Spacing and colour come from `--wa-*` tokens. No hard-coded px except the rail widths and the `46rem` container-query threshold.
- Exactly **one** scroll region per route. The frame itself never scrolls.
- Height chains use `min-height: 0` on grid children. No `height: 100%`.
- Responsiveness uses **container queries on the route**, never viewport media queries.
- The web layer draws no titlebar. The native window titlebar stands alone.
- Window minimum is 569×600 (`crates/livtet-desktop/tauri.conf.json`). Every layout must work at 569px wide.
- SSR is off app-wide (`web/routes/+layout.ts`); components may touch `document` at module scope only inside `onMount`.
- Preserve these test hooks: `.card-wrap`, `#library-search`, `.result-count`, `.suggestion-item`.
- Verify with `mise test` and `mise lint`.

## Review Focus

Input classes the spec implies that no task's happy-path tests would otherwise exercise. Each has a test assigned to the task that owns the code.

1. **Empty library (0 results).** The scroll region must still occupy its grid row and the empty state must be visible rather than collapsed to zero height. → Task 3.
2. **Toolbar overflow.** A long route title plus many filter chips must wrap, not clip or force horizontal page scroll, at 569px. → Task 3.
3. **Plugin toggle failure.** If the enable/disable IPC call rejects, the switch and badge must return to their previous state rather than showing a state the backend does not have. → Task 5.
4. **Keyboard reachability.** Rail items, toolbar controls and dock actions must all be tab-reachable, and the dock must not trap focus when selection mode is active. → Task 3.
5. **Nav rail at the 569px minimum.** The rail must be icon-only and its labels must not be read by screen readers as empty — each item keeps an accessible name. → Task 1.

---

### Task 1: Layout primitives

The `browser` Vitest project exists in `vitest.config.ts` but currently has **no tests**; these are its first. `vitest-browser-svelte` is already a devDependency.

The spec names four primitives. Implementation needs a fifth, `RouteShell`, to own the nav/pane grid and establish the container-query context — without it every route layout repeats that grid. Amend the spec, ADR-0033 and the glossary to say five.

**Files:**
- Create: `web/lib/layout/AppFrame.svelte`, `web/lib/layout/RouteShell.svelte`, `web/lib/layout/NavRail.svelte`, `web/lib/layout/Pane.svelte`, `web/lib/layout/ScrollRegion.svelte`, `web/lib/layout/Dock.svelte`, `web/lib/layout/navItems.ts`
- Test: `web/tests/browser/layout.browser.test.ts`
- Modify: `doc/superpowers/specs/2026-10-04-layout-system-design.md`, `doc/adr/0033-per-route-layouts-replacing-wa-page.md`, `doc/GLOSSARY.md`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `AppFrame` — props `{ children: Snippet }`. Renders a `100dvh` grid, `overflow: hidden`.
  - `RouteShell` — props `{ nav?: Snippet, children: Snippet }`. Grid; `container-type: inline-size`. Two columns (`auto 1fr`) when `nav` is supplied, one column otherwise.
  - `NavRail` — props `{ items: NavItem[], currentPath: string, expanded?: boolean }`. Items render as `<wa-button href=…>` so existing e2e selectors and link semantics hold. Marks the active item with `aria-current="page"`.
  - `Pane` — props `{ children: Snippet }`. Vertical stack (column flex) that grows its `ScrollRegion`, `min-height: 0`, `position: relative`.
  - `ScrollRegion` — props `{ children: Snippet, element?: HTMLElement }` (bindable `element`). A plain `overflow: auto` div carrying a `data-scroll-region` attribute, which is how tests assert the one-per-route rule.
  - `Dock` — props `{ children: Snippet }`. Absolutely positioned within the nearest `Pane`.
  - `navItems.ts` — `export type NavItem = { href: string; label: string; icon: string }` and `export const NAV_ITEMS: readonly NavItem[]` for Library, Catalogs, Plugins, Settings.

- [ ] **Step 1: Write the failing tests**

```ts
// web/tests/browser/layout.browser.test.ts
import { render } from 'vitest-browser-svelte'
import { expect, test } from 'vitest'

test('ScrollRegion is the only scrolling element and scroll events bubble')
// Render ScrollRegion with overflowing content inside a fixed-height Pane.
// Assert getComputedStyle(region).overflowY === 'auto',
// region.scrollHeight > region.clientHeight,
// and that a 'scroll' event dispatched on it reaches a listener on its parent.

test('RouteShell omits the nav column when no nav snippet is given')
// Assert gridTemplateColumns resolves to a single track.

test('NavRail marks the active item with aria-current and keeps an accessible name when collapsed')
// Render with currentPath '/library', expanded false.
// Assert the Library item has aria-current="page", that every item exposes a
// non-empty accessible name, and that label text is not rendered visibly.
// (Review Focus 5.)

test('Dock positions against its Pane, not the viewport')
// Render Dock inside an offset Pane; assert the dock's offsetParent is the pane.
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test:browser`
Expected: FAIL — the components do not exist.

- [ ] **Step 3: Implement the six components and `navItems.ts`**

Use the interface signatures above. Each component is a `<script lang="ts">` with `$props()`, markup, and a scoped `<style>`. Put the rail widths on `:root`-style custom properties local to `NavRail` (`--app-rail-width: 3.25rem`, `--app-rail-width-expanded: 11rem`). The collapse rule belongs in `NavRail`:

```css
@container (width < 46rem) {
  .rail, .rail[data-expanded] { inline-size: var(--app-rail-width); }
  .rail[data-expanded] .rail__label { display: none; }
}
```

Collapsed labels must stay in the accessibility tree — hide them visually, not with `display: none` on the only accessible name.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pnpm test:browser`
Expected: PASS, 4 tests.

- [ ] **Step 5: Amend the design docs to five primitives**

Add `RouteShell` to the primitive table in the spec, to the ADR's Decision section, and as a glossary UI entry. One sentence each: it owns the nav/pane grid and establishes the container-query context.

- [ ] **Step 6: Commit**

```bash
git add web/lib/layout web/tests/browser doc
git commit -m "feat(layout): add layout primitives"
```

---

### Task 2: Cut the shell over to AppFrame

This is the smallest change that leaves the app working without `<wa-page>`: removing it removes navigation for every route at once, so every route gains a layout here. Library and settings keep their current internals; Tasks 3 and 4 refine them.

**Files:**
- Modify: `web/routes/+layout.svelte`, `web/app.wa.js`, `crates/livtet-desktop/tauri.conf.json`, `web/routes/+page.svelte`, `web/routes/library/+page.svelte`, `web/routes/settings/+page.svelte`, `web/routes/catalog/+page.svelte`, `web/routes/plugins/+page.svelte`, `e2e/app.spec.ts`
- Create: `web/lib/layout/ListLayout.svelte`
- Test: `web/tests/browser/layout.browser.test.ts` (extend)

**Interfaces:**
- Consumes: all of Task 1.
- Produces: `ListLayout` — props `{ title: string, action?: Snippet, children: Snippet }`. Renders `RouteShell` + `NavRail` + `Pane` + toolbar + one `ScrollRegion`. Used by `/catalog` and `/plugins`.

- [ ] **Step 1: Write the failing test**

```ts
test('the app frame never scrolls and each route has exactly one scroll region')
// Render the root layout around a ListLayout with overflowing content.
// Assert the frame element's scrollHeight === clientHeight,
// and that querySelectorAll('[data-scroll-region]').length === 1.
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm test:browser`
Expected: FAIL — the root layout still renders `<wa-page>`.

- [ ] **Step 3: Rewrite `web/routes/+layout.svelte`**

Keep `QueryClientProvider`, `Toaster`, `CommandPalette`, `SvelteQueryDevtools`, the hotkey registrations and `initBindings`. Replace the `<wa-page>` block with `AppFrame` wrapping `{@render children?.()}`. Delete the `header`, `nav` and `footer` markup, the `navItems` constant (now `NAV_ITEMS`), the `isActive` helper, the `pageTitle` derivation, and the `:global(wa-page[view='desktop'])` rule.

Leave `focusSearch` in place for now — Task 3 moves it.

- [ ] **Step 4: Add `ListLayout` and give every route a layout**

`/catalog` and `/plugins` use `ListLayout`. `/library` and `/settings` get `RouteShell` + `NavRail` + `Pane` wrappers around their existing markup, with their `<main>` and its `height: 100%` removed. `/` (splash) uses `RouteShell` with no `nav` snippet.

- [ ] **Step 5: Update `web/app.wa.js`**

Remove the `page/page.js` import. Add `switch/switch.js`.

- [ ] **Step 6: Revert the overlay titlebar**

In `crates/livtet-desktop/tauri.conf.json`, remove `"hiddenTitle": true` and `"titleBarStyle": "Overlay"` from the `main` window so the window is normally decorated. Without this, on macOS the library toolbar renders beneath the traffic lights.

- [ ] **Step 7: Fix the e2e navigation selector**

`e2e/app.spec.ts:20` targets `wa-button[href="/settings"]`. `NavRail` keeps that element, so confirm rather than rewrite. Run the suite and change the selector only if it actually fails.

- [ ] **Step 8: Verify**

Run: `pnpm test:browser` → PASS.
Run: `pnpm test:e2e:browser` → PASS, or a named failure you fix before continuing.
Run: `grep -rn "wa-page" web/` → no matches.

- [ ] **Step 9: Commit**

```bash
git add web crates/livtet-desktop/tauri.conf.json e2e
git commit -m "feat(layout): replace wa-page with the AppFrame shell"
```

---

### Task 3: Library route layout

**Files:**
- Create: `web/routes/library/LibraryLayout.svelte`
- Modify: `web/routes/library/+page.svelte`, `web/routes/+layout.svelte`, `web/lib/hotkeys/commands.ts`
- Test: `web/tests/browser/library-layout.browser.test.ts`

**Interfaces:**
- Consumes: Task 1 primitives; `ListLayout` is not used here.
- Produces: `LibraryLayout` — props `{ toolbar: Snippet, strip: Snippet, children: Snippet, dock?: Snippet }`. Renders `RouteShell` + `NavRail` + `Pane` with rows `auto auto 1fr`, the third being a `ScrollRegion`, plus a `Dock` when `dock` is supplied.

- [ ] **Step 1: Write the failing tests**

```ts
test('the book grid scroll region is the IntersectionObserver root')
// Assert the sentinel's observer root is the ScrollRegion element, not null.

test('an empty library still renders its empty state at non-zero height')
// Render with zero books; assert the empty-state element's clientHeight > 0.
// (Review Focus 1.)

test('the toolbar wraps rather than clipping at 569px')
// Render at 569px with a long title and eight filter chips.
// Assert the toolbar's scrollWidth <= its clientWidth and that
// document.documentElement.scrollWidth <= its clientWidth.
// (Review Focus 2.)

test('rail, toolbar and dock controls are all tab-reachable and the dock does not trap focus')
// With selection mode on, walk focusable elements in DOM order and assert the
// rail items, toolbar buttons and dock buttons all appear. (Review Focus 4.)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test:browser`
Expected: FAIL — `LibraryLayout` does not exist.

- [ ] **Step 3: Implement `LibraryLayout` and move the library page onto it**

In `web/routes/library/+page.svelte`: replace `<wa-scroller class="book-scroller">` with the layout's `ScrollRegion`, bind its element, and pass it as the `root` of the existing `IntersectionObserver` in `sentinel`. Delete the `main { height: 100% }` rule and the `.book-scroller` rule. Move `.selection-dock` from `position: fixed` into the layout's `Dock`.

Keep the comment at `+page.svelte:233-237` only if it is still true; the `wa-scroller` explanation is not, so replace it with one line stating the observer uses the scroll region as its root.

- [ ] **Step 4: Move `search.focus` out of the shell**

Delete `focusSearch` from `web/routes/+layout.svelte` and its `document.getElementById('library-search')` call. The library route registers the handler for the `search.focus` command id instead, focusing its own input by local reference. Keep the `#library-search` id — e2e depends on it.

- [ ] **Step 5: Verify**

Run: `pnpm test:browser` → PASS.
Run: `pnpm test:e2e:browser` → PASS.

- [ ] **Step 6: Commit**

```bash
git add web
git commit -m "feat(library): move the library route onto its own layout"
```

---

### Task 4: Settings route layout

**Files:**
- Create: `web/routes/settings/SettingsLayout.svelte`
- Modify: `web/routes/settings/+page.svelte`
- Test: `web/tests/browser/settings-layout.browser.test.ts`

**Interfaces:**
- Consumes: Task 1 primitives.
- Produces: `SettingsLayout` — props `{ children: Snippet }`. Renders `RouteShell` + `NavRail` + `Pane` with a toolbar row and a `<wa-tab-group>` row. The tab group's `::part(body)` is this route's scroll region.

- [ ] **Step 1: Write the failing tests**

```ts
test('the tab body scrolls and the pane does not')
// Assert getComputedStyle(group.shadowRoot.querySelector('[part~="body"]')).overflowY === 'auto'
// and that the body scrolls while the pane's scrollHeight === its clientHeight.

test('switching tabs does not grow the pane')
// Record the pane's clientHeight, switch to a longer panel, assert it is unchanged.
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test:browser`
Expected: FAIL — `SettingsLayout` does not exist.

- [ ] **Step 3: Implement `SettingsLayout`**

Keep the existing `<wa-tab-group>` with its Sync / Catalogs / Keyboard tabs and panels. Give the group the scroll contract:

```css
.settings__tabs { display: flex; min-height: 0; }
.settings__tabs::part(base) { flex: 1 1 auto; min-height: 0; }
.settings__tabs::part(body) { flex: 1 1 auto; min-height: 0; overflow: auto; }
```

No container query is needed here: `wa-tab-group`'s nav already scrolls horizontally when narrow. Remove the `.settings { padding: var(--wa-space-xl) }` rule; padding belongs on the scroll region.

- [ ] **Step 4: Confirm the e2e heading assertion**

`e2e/app.spec.ts:22` asserts a `Keyboard` heading. Tabs are retained, so this is expected to pass — run it and confirm rather than assume.

- [ ] **Step 5: Verify**

Run: `pnpm test:browser` → PASS.
Run: `pnpm test:e2e:browser` → PASS.

- [ ] **Step 6: Commit**

```bash
git add web
git commit -m "feat(settings): give the settings layout the scroll contract"
```

---

### Task 5: Plugin enable/disable toggle

A feature, not layout. It is in this plan because the prototype surfaced it and the spec scoped it in. Check `web/lib/plugins.ts` and `web/lib/bindings.ts` for an existing enable/disable command **before** adding one; if none exists, that is an IPC change and the `adr` skill requires discussing it with the user first.

**Files:**
- Modify: `web/lib/components/PluginList.svelte`, `web/lib/plugins.ts`
- Test: `web/lib/plugins.test.ts`, `web/tests/browser/plugin-toggle.browser.test.ts`

**Interfaces:**
- Consumes: `ListLayout` from Task 2.
- Produces: `setPluginEnabled(id: string, enabled: boolean): Promise<void>` in `web/lib/plugins.ts`.

- [ ] **Step 1: Write the failing tests**

```ts
// web/lib/plugins.test.ts
test('setPluginEnabled forwards the id and enabled flag to the command')

// web/tests/browser/plugin-toggle.browser.test.ts
test('toggling a plugin updates its switch and status badge')
// Assert the badge text goes 'enabled' -> 'disabled' and the row is marked disabled.

test('a rejected toggle restores the previous switch and badge state')
// Make the command reject; assert the switch returns to checked and the badge
// reads 'enabled', and that an error toast is raised. (Review Focus 3.)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test:unit` and `pnpm test:browser`
Expected: FAIL — `setPluginEnabled` is not defined.

- [ ] **Step 3: Implement `setPluginEnabled` and the row toggle**

Add a `<wa-switch>` per plugin row in `PluginList.svelte` with `aria-label` naming the plugin. On `change`, call `setPluginEnabled`, invalidate the plugins query, and on rejection restore the prior state and raise `toast.error` — matching the error handling already used in `web/routes/library/+page.svelte`.

- [ ] **Step 4: Verify**

Run: `pnpm test` → PASS.

- [ ] **Step 5: Commit**

```bash
git add web
git commit -m "feat(plugins): add a per-plugin enable/disable toggle"
```

---

### Task 6: Full verification

- [ ] **Step 1: Run the full suite**

Run: `mise test`
Expected: PASS, Rust and JS.

- [ ] **Step 2: Run lint**

Run: `mise lint`
Expected: clean.

- [ ] **Step 3: Confirm the invariants hold in the real app**

Run: `mise dev`. Check at the 569px window minimum that the rail is icon-only, that each route scrolls in exactly one place, that the library toolbar is not under the window controls, and that the selection dock does not overlap the rail.

- [ ] **Step 4: Remove the prototype**

```bash
rm -rf prototype
```

It is untracked and throwaway; the spec records what it demonstrated.
