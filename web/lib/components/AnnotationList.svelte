<script lang="ts">
import type { HighlightColor } from '$lib/reader/highlights'
import { DEFAULT_HIGHLIGHT, HIGHLIGHT_COLORS, HIGHLIGHTS } from '$lib/reader/highlights'
import { filterNotes } from '$lib/reader/notes'
import type { Annotation } from '$lib/reader/progress'
import { formatNoteDate } from '$lib/reader/progress'

interface Props {
  notes: Annotation[]
  /** Heading text; `null` renders the list without one. */
  title?: string | null
  /**
   * Per-note actions. Each is left out of the row when omitted, so the drawer
   * gets a read-only list and the reader gets visit / edit / delete / recolour.
   */
  onvisit?: (note: Annotation) => void
  onedit?: (note: Annotation) => void
  ondelete?: (note: Annotation) => void
  onrecolor?: (note: Annotation, color: HighlightColor) => void
}

let { notes, title = 'Notes', onvisit, onedit, ondelete, onrecolor }: Props = $props()

let searchQuery = $state('')

/** Notes the search leaves visible; filtering is local and instant. */
const visibleNotes = $derived(filterNotes(notes, searchQuery))
const hasActions = $derived(Boolean(onvisit || onedit || ondelete || onrecolor))
const count = $derived(
  visibleNotes.length === notes.length
    ? `${notes.length}`
    : `${visibleNotes.length}/${notes.length}`,
)
</script>

<!--
  Marginalia: the highlighted passage keeps a narrow left column, the reader's
  own note reads beside it, and the palette is a rule rather than a filled
  block. Shared by the edition drawer and the reader's notes panel so the two
  cannot drift.
-->
<div class="annotations">
  {#if title !== null}
    <div class="annotations-head">
      <h3 class="annotations-title">{title} ({count})</h3>
      <wa-input
        class="search"
        type="search"
        size="s"
        placeholder="Search notes…"
        aria-label="Search notes"
        value={searchQuery}
        oninput={(event: Event) =>
          (searchQuery = (event.currentTarget as HTMLInputElement).value)}
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
    </div>
  {/if}

  {#if visibleNotes.length === 0 && searchQuery !== ''}
    <p class="empty">No notes match “{searchQuery}”.</p>
  {/if}

  <ul class="notes">
    {#each visibleNotes as note (note.id)}
      <li class="note" style="--note-tint: {HIGHLIGHTS[note.color ?? DEFAULT_HIGHLIGHT]?.tint}">
        {#if note.quote}
          <p class="note-quote">{note.quote}</p>
        {:else}
          <p class="note-quote is-empty">Note only</p>
        {/if}
        <div class="note-body">
          <p class="note-text">{note.text}</p>
          {#if formatNoteDate(note.createdAt)}
            <time class="note-date" datetime={note.createdAt}>
              {formatNoteDate(note.createdAt)}
            </time>
          {/if}
          {#if hasActions}
            <div class="note-actions">
              <!--
                `wa-button` keeps its interactive semantics in shadow DOM, which
                Svelte's static a11y checks cannot see. Same suppression as
                ActionButton.svelte.
              -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <wa-button-group>
                {#if onvisit}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <wa-button size="s" onclick={() => onvisit(note)}>Visit</wa-button>
                {/if}
                {#if onedit}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <wa-button size="s" onclick={() => onedit(note)}>Edit</wa-button>
                {/if}
                {#if ondelete}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <wa-button size="s" onclick={() => ondelete(note)}>
                    <wa-icon name="trash"></wa-icon>
                    Delete
                  </wa-button>
                {/if}
              </wa-button-group>
              {#if onrecolor}
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
              {/if}
            </div>
          {/if}
        </div>
      </li>
    {/each}
  </ul>
</div>

<style>
.annotations {
display: flex;
flex-direction: column;
gap: var(--wa-space-s, 0.75rem);
container-type: inline-size;
}

.annotations-head {
display: flex;
align-items: center;
gap: var(--wa-space-s, 0.75rem);
}

.annotations-title {
flex: 0 0 auto;
margin: 0;
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-2xs, 0.72rem);
font-weight: 600;
letter-spacing: 0.06em;
text-transform: uppercase;
}

.search {
flex: 1 1 auto;
--wa-input-width: 100%;
}

.empty {
margin: 0;
padding: var(--wa-space-2xs, 0.25rem) 0;
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-s, 0.9rem);
}

.notes {
margin: 0;
padding: 0;
list-style: none;
}

/* The passage keeps a column of its own and the note reads beside it. */
.note {
display: grid;
grid-template-columns: minmax(10rem, 38%) minmax(0, 1fr);
gap: var(--wa-space-m, 1rem);
padding-block: var(--wa-space-s, 0.75rem);
border-block-start: 1px solid var(--wa-color-surface-border);
}

.note:first-child {
padding-block-start: 0;
border-block-start: 0;
}

.note-quote {
margin: 0;
padding-inline-start: var(--wa-space-xs, 0.5rem);
border-inline-start: 3px solid var(--note-tint, var(--wa-color-surface-border));
color: var(--wa-color-text-quiet);
font-family: var(--wa-font-family-heading);
font-size: 0.9rem;
font-style: italic;
line-height: 1.4;
text-wrap: pretty;
}

.note-quote.is-empty {
border-inline-start-color: var(--wa-color-surface-border);
font-size: var(--wa-font-size-2xs, 0.72rem);
font-style: normal;
}

.note-body {
display: flex;
flex-direction: column;
gap: var(--wa-space-3xs, 0.125rem);
min-width: 0;
}

.note-text {
margin: 0;
overflow-wrap: anywhere;
}

.note-date {
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-2xs, 0.72rem);
}

.note-actions {
display: flex;
flex-wrap: wrap;
align-items: center;
gap: var(--wa-space-xs, 0.5rem);
margin-block-start: var(--wa-space-2xs, 0.25rem);
}

.swatches {
display: flex;
gap: 0.25rem;
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
border-color: var(--wa-color-text-normal);
}

.swatch:hover,
.swatch:focus-visible {
border-color: var(--wa-color-surface-border);
}

/* A narrow host — the reader's floating panel, or a small drawer — cannot hold
   two columns, so the note goes back under its passage. */
@container (max-width: 30rem) {
.note {
grid-template-columns: minmax(0, 1fr);
gap: var(--wa-space-3xs, 0.125rem);
}
}
</style>
