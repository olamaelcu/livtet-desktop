<script lang="ts">
import type { Chapter } from '$lib/reader/chapters'

interface Props {
  chapters: Chapter[]
  /** Resource on screen; its row is marked current. */
  currentHref: string
  /** Move the reader to the chapter's resource. */
  onselect: (chapter: Chapter) => void
  /** Dismiss the drawer, whether by a chapter, its Close button, or Escape. */
  onclose: () => void
}

let { chapters, currentHref, onselect, onclose }: Props = $props()
</script>

<!--
  The chapter list as a right-hand drawer over the reading pane, so the page
  stays visible behind it. A chapter is one resource of the publication's
  reading order: choosing one moves the reader to its start and closes the
  drawer, which would otherwise cover the text it just went to.
-->
<wa-drawer placement="end" open onwa-after-hide={onclose}>
  {#if chapters.length === 0}
    <p class="empty">This book names no chapters.</p>
  {:else}
    <ul class="chapters">
      {#each chapters as chapter (chapter.href)}
        <li>
          <a
            href="#"
            class="chapter"
            class:current={chapter.href === currentHref}
            aria-current={chapter.href === currentHref ? 'true' : undefined}
            onclick={() => onselect(chapter)}
          >
            <span class="title">{chapter.title}</span>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</wa-drawer>

<style>
  .chapters {
    display: flex;
    flex-direction: column;
    text-align: left;
    gap: var(--wa-space-3xs);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .chapter {
    display: inline-flex;
    gap: var(--wa-space-s);
    width: 100%;
    padding: var(--wa-space-2xs) var(--wa-space-s);
    border: none;
    border-radius: var(--wa-border-radius-s, 0.25rem);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .chapter:hover {
    background: var(--wa-color-bg-hover);
  }

  .chapter.current {
    background: var(--wa-color-brand-fill-quiet);
    font-weight: var(--wa-font-weight-semibold);
  }

  /* Long chapter titles wrap rather than being cut: the drawer is a list to
     read, and an ellipsis would hide which of two similar titles is which. */
  .title {
    overflow-wrap: anywhere;
  }

  .empty {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }
</style>
