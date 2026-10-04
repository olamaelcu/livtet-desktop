<script lang="ts">
import type { Snippet } from 'svelte'
import { page } from '$app/state'
import Dock from '../../lib/layout/Dock.svelte'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import Pane from '../../lib/layout/Pane.svelte'
import RouteShell from '../../lib/layout/RouteShell.svelte'
import ScrollRegion from '../../lib/layout/ScrollRegion.svelte'

interface Props {
  toolbar: Snippet
  strip: Snippet
  children: Snippet
  dock?: Snippet
  /** Floating UI (popovers, dialogs, drawers) that belongs to the pane. */
  overlays?: Snippet
  /** The scroll region element, for rooting an IntersectionObserver. */
  scrollElement?: HTMLElement
}

let { toolbar, strip, children, dock, overlays, scrollElement = $bindable() }: Props = $props()
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} expanded />
  {/snippet}
  <Pane>
    {@render toolbar()}
    {@render strip()}
    <ScrollRegion bind:element={scrollElement}>
      {@render children()}
    </ScrollRegion>
    {#if dock}
      <Dock>{@render dock()}</Dock>
    {/if}
    {@render overlays?.()}
  </Pane>
</RouteShell>
