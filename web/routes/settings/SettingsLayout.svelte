<script lang="ts">
import type { Snippet } from 'svelte'
import { page } from '$app/state'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import Pane from '../../lib/layout/Pane.svelte'
import RouteShell from '../../lib/layout/RouteShell.svelte'

/** `children` are the `<wa-tab-panel>`s for the sync, catalogs and hotkeys tabs. */
let { children }: { children: Snippet } = $props()
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} expanded />
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
    <wa-tab-group class="settings__tabs" data-scroll-region>
      <wa-tab slot="nav" panel="sync">Sync</wa-tab>
      <wa-tab slot="nav" panel="catalogs">Catalogs</wa-tab>
      <wa-tab slot="nav" panel="hotkeys">Keyboard</wa-tab>
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
