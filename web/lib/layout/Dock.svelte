<script lang="ts">
import type { Snippet } from 'svelte'

let { children }: { children: Snippet } = $props()
</script>

<div class="dock" data-dock>
  {@render children()}
</div>

<style>
  .dock {
    position: absolute;
    inset-inline: var(--wa-space-m);
    inset-block-end: var(--wa-space-m);
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-m);
    background: var(--wa-color-surface-raised);
    border: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
    border-radius: var(--wa-border-radius-l);
    box-shadow: var(--wa-shadow-l);
  }
</style>
