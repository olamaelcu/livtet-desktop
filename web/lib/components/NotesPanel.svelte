<script lang="ts">
import AnnotationList from '$lib/components/AnnotationList.svelte'
import type { Bookmark } from '$lib/reader/bookmark'
import { bookmarkLabel } from '$lib/reader/bookmark'
import type { HighlightColor } from '$lib/reader/highlights'
import type { Annotation } from '$lib/reader/progress'
import { formatNoteDate } from '$lib/reader/progress'

interface Props {
  notes: Annotation[]
  /** Page bookmarks for this edition; listed apart from notes, without text. */
  bookmarks: Bookmark[]
  /** Move the reader to the note's location. */
  onvisit: (note: Annotation) => void
  /** Open the composer on this note. */
  onedit: (note: Annotation) => void
  ondelete: (note: Annotation) => void
  onrecolor: (note: Annotation, color: HighlightColor) => void
  /** Move the reader to the bookmark's location. */
  onvisitbookmark: (bookmark: Bookmark) => void
  /** Drop the bookmark; the page itself is unaffected. */
  onremovebookmark: (bookmark: Bookmark) => void
  onclose: () => void
}

let {
  notes,
  bookmarks,
  onvisit,
  onedit,
  ondelete,
  onrecolor,
  onvisitbookmark,
  onremovebookmark,
  onclose,
}: Props = $props()
</script>

<!--
  The reader's floating notes surface: panel chrome plus bookmarks. The notes
  themselves are `AnnotationList`, the same component the edition drawer uses,
  so a note reads identically in both places.
-->
<div class="panel" role="dialog" aria-label="Notes and bookmarks">
  <div class="panel-heading">
    <span>Notes and bookmarks</span>
    <!--
      `wa-button` keeps its interactive semantics in shadow DOM, which Svelte's
      static a11y checks cannot see. Same suppression as ActionButton.svelte.
    -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <wa-button size="s" onclick={onclose}>
      <wa-icon name="xmark"></wa-icon>
    </wa-button>
  </div>

  {#if bookmarks.length > 0}
    <p class="section-heading">Bookmarks ({bookmarks.length})</p>
    {#each bookmarks as bookmark (bookmark.id)}
      <div class="bookmark-row">
        <div class="bookmark-body">
          <span class="bookmark-label">{bookmarkLabel(bookmark)}</span>
          {#if formatNoteDate(bookmark.createdAt)}
            <time class="note-date" datetime={bookmark.createdAt}>
              {formatNoteDate(bookmark.createdAt)}
            </time>
          {/if}
        </div>
        <div class="row-actions">
          <wa-button-group orientation="vertical">
            <!--
              `wa-button` keeps its interactive semantics in shadow DOM, which
              Svelte's static a11y checks cannot see. Same suppression as
              ActionButton.svelte.
            -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <wa-button size="s" onclick={() => onvisitbookmark(bookmark)}>Visit</wa-button>
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <wa-button size="s" onclick={() => onremovebookmark(bookmark)}>
              <wa-icon name="trash"></wa-icon>
              Remove
            </wa-button>
          </wa-button-group>
        </div>
      </div>
    {/each}
  {/if}

  <AnnotationList {notes} {onvisit} {onedit} {ondelete} {onrecolor} />
</div>

<style>
  .panel {
    position: absolute;
    z-index: 2;
    inset-block-end: var(--wa-space-m);
    inset-inline-start: var(--wa-space-m);
    width: min(34rem, 80vw);
    max-height: 40%;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-surface-border);
    background: var(--wa-color-surface-default);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
  }

  .panel-heading {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-xs);
    margin: 0 0 var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs, 0.8125rem);
    color: var(--wa-color-text-quiet);
  }

  .section-heading {
    margin: var(--wa-space-s) 0 var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs, 0.8125rem);
    color: var(--wa-color-text-quiet);
  }

  .bookmark-row {
    display: flex;
    flex-direction: row;
    gap: var(--wa-space-2xs);
    padding-block: var(--wa-space-2xs);
    border-block-start: 1px solid var(--wa-color-surface-border);
  }

  .bookmark-body {
    flex: 1 1 auto;
    min-width: 0;
  }

  .bookmark-label {
    display: block;
    padding: var(--wa-space-2xs);
  }

  .note-date {
    display: block;
    margin-block-start: var(--wa-space-2xs);
    font-size: var(--wa-font-size-2xs, 0.75rem);
    color: var(--wa-color-text-quiet);
  }

  .row-actions {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--wa-space-2xs);
  }
</style>
