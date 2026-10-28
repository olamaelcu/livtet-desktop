<script lang="ts">
import type { Annotation } from '$lib/reader/progress'
import ActionButton from './ActionButton.svelte'

interface Props {
  /** The annotation whose highlight the reader clicked. */
  annotation: Annotation
  onclose: () => void
}

let { annotation, onclose }: Props = $props()
</script>

<!--
  A clicked highlight opens this sheet across the head of the reading pane. It
  sits there rather than at the foot because the reader bar and the note list
  both anchor to the foot, and either would clip it.
-->
<div class="drawer" role="dialog" aria-label="Annotation">
  {#if annotation.quote}
    <blockquote class="quote">{annotation.quote}</blockquote>
  {/if}
  {#if annotation.text}
    <p class="body">{annotation.text}</p>
  {/if}
  <div class="actions">
    <ActionButton size="small" onclick={onclose}>Close</ActionButton>
  </div>
</div>

<style>
  .drawer {
    position: absolute;
    z-index: 3;
    inset-block-start: var(--wa-space-m);
    inset-inline: var(--wa-space-m);
    max-height: 40%;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-border-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-border);
    background: var(--wa-color-surface-default);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
  }

  .quote {
    display: block;
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .body {
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
    white-space: pre-wrap;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
