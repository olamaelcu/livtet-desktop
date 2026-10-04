<script lang="ts">
import type { Snippet } from 'svelte'

let { children }: { children: Snippet } = $props()
</script>

<div class="pane">
  {@render children()}
</div>

<style>
  /*
   * A vertical stack whose final child takes the remaining height. CSS grid
   * cannot give "the last of N auto rows" 1fr without knowing N, so this uses
   * a column flexbox; the min-height: 0 discipline is the same.
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

  .pane > :global(:last-child) {
    flex: 1 1 0;
    min-height: 0;
  }
</style>
