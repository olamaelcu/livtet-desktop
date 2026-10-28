<script lang="ts">
import type { Bookmark } from '$lib/reader/bookmark'
import { bookmarkLabel } from '$lib/reader/bookmark'
import type { HighlightColor } from '$lib/reader/highlights'
import { DEFAULT_HIGHLIGHT, HIGHLIGHT_COLORS, HIGHLIGHTS } from '$lib/reader/highlights'
import { filterNotes } from '$lib/reader/notes'
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

let searchQuery = $state('')

/** Notes the search leaves visible; filtering is local and instant. */
const visibleNotes = $derived(filterNotes(notes, searchQuery))
</script>

<!--
  Floats over the reading pane rather than taking layout space. Each row is a
  note with its quote, text, date, colour swatches, and the actions that act on
  it; the reader never leaves the page to manage notes. Bookmarks are their own
  list because they hold neither text nor colour.
-->
<div class="panel" role="dialog" aria-label="Notes and bookmarks">
  <div class="panel-heading">
    <span>
      Notes ({visibleNotes.length}{#if visibleNotes.length !== notes.length}/{notes.length}{/if})
    </span>
    <wa-input
      class="search"
      type="search"
      placeholder="Search notes..."
      size="s"
      aria-label="Search notes"
      value={searchQuery}
      oninput={(event: Event) => {
        searchQuery = (event.target as HTMLInputElement).value
      }}
    >
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <wa-button
        slot="end"
        size="s"
        variant={searchQuery ? 'brand' : 'neutral'}
        disabled={searchQuery.length === 0}
        aria-label="Clear search"
        title="Clear search"
        onclick={() => (searchQuery = '')}
      >
        <wa-icon name={searchQuery ? 'xmark' : 'search'}></wa-icon>
      </wa-button>
    </wa-input>
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

  {#if visibleNotes.length === 0 && searchQuery !== ''}
    <p class="empty">No notes match “{searchQuery}”.</p>
  {/if}

  {#each visibleNotes as note (note.id)}
    <div class="note-row">
      <div class="note-body">
        {#if note.quote}
          <span
            class="note-quote"
            style:background={HIGHLIGHTS[note.color ?? DEFAULT_HIGHLIGHT]?.tint}>{note.quote}</span
          >
        {/if}
        <span class="note-text">{note.text}</span>
        {#if formatNoteDate(note.createdAt)}
          <time class="note-date" datetime={note.createdAt}>
            {formatNoteDate(note.createdAt)}
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
          <wa-button size="s" onclick={() => onvisit(note)}>Visit</wa-button>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <wa-button size="s" onclick={() => onedit(note)}>Edit</wa-button>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <wa-button size="s" onclick={() => ondelete(note)}>
            <wa-icon name="trash"></wa-icon>
            Delete
          </wa-button>
        </wa-button-group>
        <div class="swatches" role="group" aria-label="Highlight colour">
          {#each HIGHLIGHT_COLORS as key (key)}
            <button
              type="button"
              class="swatch"
              class:selected={(note.color ?? DEFAULT_HIGHLIGHT) === key}
              style:background={HIGHLIGHTS[key].tint}
              title={HIGHLIGHTS[key].label}
              aria-label={HIGHLIGHTS[key].label}
              aria-pressed={(note.color ?? DEFAULT_HIGHLIGHT) === key}
              onclick={() => onrecolor(note, key)}
            ></button>
          {/each}
        </div>
      </div>
    </div>
  {/each}
</div>

<style>
  .panel {
    position: absolute;
    z-index: 2;
    inset-block-end: var(--wa-space-m);
    inset-inline-start: var(--wa-space-m);
    max-width: 80vw;
    max-height: 40%;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-border-default);
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
    color: var(--wa-color-text-secondary);

    .search {
      flex: 1 1;

      [slot=end] {
        position: relative;
        left: var(--wa-space-m);
      }
    }
  }

  .section-heading {
    margin: var(--wa-space-s) 0 var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs, 0.8125rem);
    color: var(--wa-color-text-secondary);
  }

  .empty {
    margin: var(--wa-space-s) 0;
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-s);
  }

  .bookmark-row,
  .note-row {
    display: flex;
    flex-direction: row;
    gap: var(--wa-space-2xs);
    padding-block: var(--wa-space-2xs);
    border-block-start: 1px solid var(--wa-color-border-default);
  }

  .bookmark-body,
  .note-body {
    flex: 1 1 auto;
    min-width: 0;
  }

  .bookmark-label {
    display: block;
    padding: var(--wa-space-2xs);
  }

  .note-body:hover,
  .note-body:focus-visible {
    background: var(--wa-color-surface-lowered, rgb(0 0 0 / 0.04));
  }

  .note-quote {
    display: block;
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border-default);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .note-text {
    display: block;
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
  }

  .note-date {
    display: block;
    margin-block-start: var(--wa-space-2xs);
    font-size: var(--wa-font-size-2xs, 0.75rem);
    color: var(--wa-color-text-muted);
  }

  .row-actions {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--wa-space-2xs);
  }

  .swatches {
    display: flex;
    gap: 0.25rem;
    margin: var(--wa-space-2xs) 0;
  }

  .swatch {
    width: 0.9rem;
    height: 0.9rem;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch.selected {
    border-color: var(--wa-color-text-default);
  }

  .swatch:hover,
  .swatch:focus-visible {
    border-color: var(--wa-color-border-default);
  }
</style>
