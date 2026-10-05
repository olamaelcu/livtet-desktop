<script lang="ts">
import type { Snippet } from 'svelte'
import { page } from '$app/state'
import NavRail from '../../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../../lib/layout/navItems'
import Pane from '../../../lib/layout/Pane.svelte'
import RouteShell from '../../../lib/layout/RouteShell.svelte'
import ScrollRegion from '../../../lib/layout/ScrollRegion.svelte'

/** `toolbar` is the search/back row; `children` fill the single scroll region. */
let { toolbar, children }: { toolbar: Snippet; children: Snippet } = $props()
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} expanded />
  {/snippet}
  <Pane>
    <header class="toolbar">
      {@render toolbar()}
    </header>
    <ScrollRegion>
      <div class="content">
        {@render children()}
      </div>
    </ScrollRegion>
  </Pane>
</RouteShell>

<style>
  .toolbar {
    display: flex;
    align-items: flex-end;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-l);
    border-block-end: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
  }

  .toolbar :global(wa-input) {
    flex: 1 1;
    max-width: 40rem;
  }

  .content {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
    padding: var(--wa-space-l);
  }
</style>
