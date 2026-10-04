<script lang="ts">
import type { Snippet } from 'svelte'

let { nav, children }: { nav?: Snippet; children: Snippet } = $props()
</script>

<div class="shell" data-route-shell data-has-nav={nav ? '' : undefined}>
  {#if nav}
    {@render nav()}
  {/if}
  {@render children()}
</div>

<style>
  .shell {
    container-type: inline-size;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    min-height: 0;
    min-width: 0;
  }

  .shell[data-has-nav] {
    grid-template-columns: auto minmax(0, 1fr);
  }
</style>
