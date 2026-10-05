<script lang="ts">
import type { Snippet } from 'svelte'
import Pane from '../../lib/layout/Pane.svelte'
import RouteShell from '../../lib/layout/RouteShell.svelte'

/**
 * Reader routes open in their own Tauri window, so application navigation is
 * deliberately absent: RouteShell gets no `nav` snippet. Each route supplies its
 * own header and controls (route chrome) plus one `data-scroll-region` child.
 */
let { children }: { children: Snippet } = $props()
</script>

<RouteShell>
  <Pane>
    {@render children()}
  </Pane>
</RouteShell>
