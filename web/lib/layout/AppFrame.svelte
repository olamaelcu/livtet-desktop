<script lang="ts">
import type { Snippet } from 'svelte'

let { children }: { children: Snippet } = $props()
</script>

<div class="frame">
  {@render children()}
</div>

<style>
  .frame {
    display: grid;
    grid-template: minmax(0, 1fr) / minmax(0, 1fr);
    block-size: 100dvh;
    overflow: hidden;
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-normal);
  }
</style>
