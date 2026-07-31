<script lang="ts">
import { formatForDisplay, getHeldKeys } from '@tanstack/svelte-hotkeys'

const held = getHeldKeys()
</script>

{#if held.keys.length > 0}
  <div class="held-keys" aria-live="polite">
    {#each held.keys as key (key)}
      <kbd>{formatForDisplay(key)}</kbd>
    {/each}
  </div>
{/if}

<style>
  .held-keys {
    position: fixed;
    bottom: 1rem;
    right: 1rem;
    display: flex;
    gap: 0.25rem;
    padding: 0.375rem 0.625rem;
    background: var(--wa-color-surface-default, white);
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 9999px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
    z-index: 20;
  }

  kbd {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.75rem;
    padding: 0.125rem 0.375rem;
    background: var(--wa-color-neutral-fill-quiet, rgba(0, 0, 0, 0.05));
    border-radius: 4px;
  }
</style>
