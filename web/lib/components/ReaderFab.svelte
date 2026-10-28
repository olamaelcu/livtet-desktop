<script lang="ts">
import ActionButton from './ActionButton.svelte'

interface Props {
  /** Formatted progress through the publication. */
  progress: string
  /** Number of notes on the publication. */
  count: number
  /** Number of page bookmarks on the publication. */
  bookmarks: number
  /** Whether the note list is open. */
  open: boolean
  /** Whether anything layered over the reader is showing. */
  expanded: boolean
  ontoggle: () => void
}

let { progress, count, bookmarks, open, expanded, ontoggle }: Props = $props()
</script>

<!--
  Floats at the foot of the reading pane: the note list button carries the live
  progress so the reader never has to leave the page to check either.
-->
<div class="fab">
  <span class="progress">{progress}</span>
  <ActionButton
    variant="brand"
    onclick={ontoggle}
    aria-pressed={open}
    aria-haspopup="dialog"
    aria-expanded={expanded}
  >
    <wa-icon name="comment-dots"></wa-icon>
    {#if count > 0}
      {count} Note{#if count !== 1}s{/if}
    {/if}
    {#if count > 0 && bookmarks > 0}
      <span class="separator">·</span>
    {/if}
    {#if bookmarks > 0}
      {bookmarks} Bookmark{#if bookmarks !== 1}s{/if}
    {/if}
  </ActionButton>
</div>

<style>
  .fab {
    position: absolute;
    inset-block-end: var(--wa-space-m);
    inset-inline-end: var(--wa-space-m);
    z-index: 2;
    width: 95%;
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: end;

    .progress {
      justify-self: center;
      flex: 1 1;
    }

    .separator {
      opacity: 0.7;
    }
  }
</style>
