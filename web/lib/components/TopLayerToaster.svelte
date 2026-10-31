<script lang="ts">
import { onMount } from 'svelte'
import { Toaster } from 'svelte-sonner'

/**
 * Toasts sit above `wa-dialog` and `wa-drawer`.
 *
 * Those components open the native `<dialog>` with `showModal()`, which paints
 * in the browser's top layer — above every `z-index`, however large (the
 * library already uses `999999999`). The only way to paint above a top-layer
 * element is to be in the top layer too, so the toaster lives in a manual
 * popover.
 *
 * Top-layer elements stack in the order they were opened, so every time a
 * dialog or drawer opens the popover is hidden and re-shown: that puts it back
 * on top, and toasts fired while the dialog is up stay visible.
 */
let layer = $state<HTMLElement>()

function raise() {
  if (!layer?.isConnected) return
  try {
    if (layer.matches(':popover-open')) layer.hidePopover()
    layer.showPopover()
  } catch {
    // No popover API: the toaster keeps the library's fixed, z-indexed layout.
  }
}

/**
 * `wa-show` fires before the component calls `showModal()`, so re-showing the
 * popover inside the handler would still leave the dialog above it. The frame
 * after is when the dialog is in the top layer and the popover can go on top.
 */
function raiseOverOpenedOverlay() {
  requestAnimationFrame(raise)
}

onMount(() => {
  raise()
  // Capture, so an overlay opened inside another one still reaches us.
  document.addEventListener('wa-show', raiseOverOpenedOverlay, true)
  document.addEventListener('wa-after-show', raiseOverOpenedOverlay, true)
  return () => {
    document.removeEventListener('wa-show', raiseOverOpenedOverlay, true)
    document.removeEventListener('wa-after-show', raiseOverOpenedOverlay, true)
  }
})
</script>

<div class="top-layer-toaster" popover="manual" bind:this={layer}>
  <Toaster theme="system" position="bottom-left" />
</div>

<style>
  /* A popover's own box is only a host for the top layer: the toaster inside
     keeps the library's fixed positioning, and the host must never intercept
     pointer events aimed at the page or the dialog underneath. */
  .top-layer-toaster {
    position: fixed;
    inset: 0;
    width: auto;
    height: auto;
    max-width: none;
    max-height: none;
    margin: 0;
    padding: 0;
    border: 0;
    background: transparent;
    overflow: visible;
    pointer-events: none;
  }

  .top-layer-toaster :global([data-sonner-toaster]) {
    pointer-events: auto;
  }
</style>
