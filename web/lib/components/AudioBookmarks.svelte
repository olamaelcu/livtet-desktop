<script lang="ts">
import ActionButton from '$lib/components/ActionButton.svelte'
import { formatTimestamp } from '$lib/reader'
import type { Bookmark } from '$lib/reader/bookmark'

interface Props {
  /** Playback spots stored for this recording, oldest first. */
  bookmarks: Bookmark[]
  /** Whether the Add button may start a save: list loaded, none in flight. */
  canAdd: boolean
  /** Whether the spot on screen already carries a bookmark. */
  bookmarked: boolean
  /** Bookmark the spot on screen. */
  onadd: () => void
  /** Move playback to the spot. */
  onvisit: (bookmark: Bookmark) => void
  /** Drop the spot; playback is unaffected. */
  onremove: (bookmark: Bookmark) => void
}

let { bookmarks, canAdd, bookmarked, onadd, onvisit, onremove }: Props = $props()
</script>

<!--
  Audiobook bookmarks are playback spots, not notes: no text and no colour, so
  they are their own list apart from the listening position. The button only
  ever adds — a bookmarked spot has it disabled, and this list owns removal.
-->
<section class="bookmarks">
  <ActionButton
    onclick={onadd}
    disabled={!canAdd || bookmarked}
    variant={bookmarked ? 'brand' : undefined}
    aria-pressed={bookmarked}
  >
    <wa-icon name="bookmark"></wa-icon>
    {bookmarked ? 'Bookmarked' : 'Bookmark this spot'}
  </ActionButton>
  {#if bookmarks.length > 0}
    <h3 class="section-title">Bookmarks ({bookmarks.length})</h3>
    <ul class="bookmark-list">
      {#each bookmarks as bookmark (bookmark.id)}
        <li class="bookmark">
          <button
            type="button"
            class="bookmark-jump"
            onclick={() => onvisit(bookmark)}
            aria-label="Play from {formatTimestamp(bookmark.positionSeconds ?? 0)}"
          >
            {formatTimestamp(bookmark.positionSeconds ?? 0)}
          </button>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <wa-button size="s" onclick={() => onremove(bookmark)}>
            <wa-icon name="trash"></wa-icon>
            Remove
          </wa-button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .bookmarks {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--wa-space-2xs);
  }

  .section-title {
    margin: var(--wa-space-s) 0 0;
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .bookmark-list {
    width: 100%;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .bookmark {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding-block: var(--wa-space-2xs);
    border-block-start: 1px solid var(--wa-color-border);
  }

  .bookmark-jump {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
</style>
