<script lang="ts">
import type { Snippet } from 'svelte'
import { replaceState } from '$app/navigation'
import { page } from '$app/state'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import Pane from '../../lib/layout/Pane.svelte'
import RouteShell from '../../lib/layout/RouteShell.svelte'
import { SETTINGS_TABS, settingsTabFrom, settingsTabUrl } from '../../lib/settings/tabs'

/** `children` are the `<wa-tab-panel>`s for the sync, catalogs, account, hotkeys, MCP, and OPDS tabs. */
let { children }: { children: Snippet } = $props()

/**
 * The tab on screen: the one `?tab=` names, or the first when it names none.
 *
 * The query string is the source of truth rather than local state, so a reload
 * or a link into the route opens the same tab the address bar asks for.
 */
const active = $derived(settingsTabFrom(page.url.search) ?? SETTINGS_TABS[0]?.name ?? '')

/**
 * The tab name `wa-tab-show` carries, or `undefined` for an event of another
 * shape.
 *
 * WebAwesome dispatches its own event class here — a plain `Event` carrying a
 * `detail` — so testing for `CustomEvent` first would discard every one of
 * them silently.
 */
function shownTab(event: Event): string | undefined {
  if (!('detail' in event)) return undefined
  const detail: unknown = event.detail
  if (typeof detail !== 'object' || detail === null || !('name' in detail)) return undefined
  return typeof detail.name === 'string' ? detail.name : undefined
}

/**
 * Point the address bar at the tab the reader switched to.
 *
 * `replaceState` rather than a navigation: the panels are already on screen, so
 * switching is not a new page and must not stack history entries.
 */
function tabShown(event: Event) {
  const name = shownTab(event)
  if (name === undefined || name === active) return
  replaceState(settingsTabUrl(page.url, name), page.state)
}
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} />
  {/snippet}
  <Pane>
    <header class="toolbar">
      <h1 class="title">Settings</h1>
    </header>
    <!--
      wa-tab-group is this route's scroll region. data-scroll-region sits on the
      host so Pane grows it; the scrolling happens on its body part, so the tab
      strip stays pinned. wa-tab-group has no scroller of its own (its body part
      computes overflow-y: visible; asserted in settings-layout.browser.test.ts),
      so this is the only one.
    -->
    <wa-tab-group class="settings__tabs" data-scroll-region {active} onwa-tab-show={tabShown}>
      {#each SETTINGS_TABS as tab (tab.name)}
        <wa-tab slot="nav" panel={tab.name}>{tab.label}</wa-tab>
      {/each}
      {@render children()}
    </wa-tab-group>
  </Pane>
</RouteShell>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--wa-space-m);
    padding: var(--wa-space-s) var(--wa-space-l);
    border-block-end: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-l);
  }

  .settings__tabs {
    display: flex;
    min-height: 0;
    min-width: 0;
  }

  .settings__tabs::part(base) {
    flex: 1 1 auto;
    min-height: 0;
    min-width: 0;
  }

  .settings__tabs::part(body) {
    flex: 1 1 auto;
    min-height: 0;
    overflow: auto;
    padding: var(--wa-space-xl);
  }
</style>
