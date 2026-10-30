<script lang="ts">
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { type Snippet } from 'svelte'

let { nav, children }: { nav?: Snippet; children: Snippet } = $props()
const isMacOs = /Mac/.test(navigator.platform)

async function startDrag() {
  await getCurrentWebviewWindow().startDragging()
}
</script>

<div class="shell" data-route-shell data-has-nav={nav ? '' : undefined} class:macos={isMacOs}>
  {#if isMacOs}
    <div tabindex=0 role="menu" class="drag-region" onmousedown={startDrag} aria-label="Drag window"></div>
  {/if}

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

  .macos.shell {
    padding-top: 2rem;
  }

  .drag-region {
    position: absolute;
    inset-block-start: 0;
    inset-inline-start: 0;
    block-size: 2rem;
    inline-size: 100%;
    cursor: grab;
    z-index: 9999;
  }
  .drag-region:active {
    cursor: grabbing;
  }

  .shell[data-has-nav] {
    grid-template-columns: auto minmax(0, 1fr);
  }
</style>
