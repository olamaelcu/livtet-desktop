<script lang="ts">
import type { Snippet } from 'svelte'
import { page } from '$app/state'
import NavRail from './NavRail.svelte'
import { NAV_ITEMS } from './navItems'
import Pane from './Pane.svelte'
import RouteShell from './RouteShell.svelte'
import ScrollRegion from './ScrollRegion.svelte'

interface Props {
  title: string
  action?: Snippet
  children: Snippet
}

let { title, action, children }: Props = $props()
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} expanded />
  {/snippet}
  <Pane>
    <header class="toolbar">
      <h1 class="title">{title}</h1>
      {#if action}
        {@render action()}
      {/if}
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
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
    padding: var(--wa-space-s) var(--wa-space-l);
    border-block-end: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-l);
  }

  .content {
    padding: var(--wa-space-l);
  }
</style>
