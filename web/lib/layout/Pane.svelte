<script lang="ts">
import type { Snippet } from 'svelte'

let { children }: { children: Snippet } = $props()
</script>

<div class="pane">
  {@render children()}
</div>

<style>
  /*
   * A vertical stack whose ScrollRegion takes the remaining height. CSS grid
   * cannot give "the last of N auto rows" 1fr without knowing N, so this uses
   * a column flexbox; the min-height: 0 discipline is the same. The pane grows
   * its ScrollRegion specifically, so a Dock may sit anywhere in source order.
   */
  .pane {
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }

  .pane > :global(*) {
    flex: 0 0 auto;
  }

  .pane > :global([data-scroll-region]) {
    flex: 1 1 0;
    min-height: 0;
  }
</style>
