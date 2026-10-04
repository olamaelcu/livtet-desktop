<script lang="ts">
import type { Snippet } from 'svelte'

let { children, element = $bindable() }: { children: Snippet; element?: HTMLElement } = $props()
</script>

<div class="scroll-region" data-scroll-region bind:this={element}>
  {@render children()}
</div>

<style>
  .scroll-region {
    overflow: auto;
    min-height: 0;
    min-width: 0;
  }
</style>
