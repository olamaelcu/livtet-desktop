<script lang="ts">
import { untrack } from 'svelte'
import type { HighlightColor } from '$lib/reader/highlights'
import { DEFAULT_HIGHLIGHT, HIGHLIGHT_COLORS, HIGHLIGHTS } from '$lib/reader/highlights'
import ActionButton from './ActionButton.svelte'

interface Props {
  /** Placeholder for the note field. */
  placeholder: string
  /** Text selected in the publication, shown as a quoted preview. */
  quote?: string
  /** Whether to offer the highlight palette. Only readers that can paint it. */
  showColors?: boolean
  /** Existing note text, when the dialog edits a note instead of creating one. */
  initialText?: string
  /** Existing highlight colour, likewise. */
  initialColor?: HighlightColor
  /** Called with the trimmed note text and the chosen highlight colour. */
  onsave: (text: string, color?: HighlightColor) => void
  oncancel: () => void
}

let {
  placeholder,
  quote,
  showColors = false,
  initialText,
  initialColor,
  onsave,
  oncancel,
}: Props = $props()

/** Editing an existing note when the caller supplies its text. */
const editing = $derived(initialText !== undefined)

// Seeded once per mount: the dialog is remounted for every open or edit, so
// these must not follow later prop changes. `untrack` states that intent.
let draft = $state(untrack(() => initialText ?? ''))
let color = $state<HighlightColor>(untrack(() => initialColor ?? DEFAULT_HIGHLIGHT))

function submit() {
  const text = draft.trim()
  onsave(text, showColors ? color : undefined)
}

/**
 * Enter saves, Escape cancels.
 *
 * `wa-input` is a custom element, so the a11y checker needs an explicit role
 * and a tabindex before it will accept the keydown handler.
 */
function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter') {
    event.preventDefault()
    submit()
  } else if (event.key === 'Escape') {
    oncancel()
  }
}
</script>

<!--
  A modal dialog, not an inline panel: opening it no longer reflows the reading
  pane, so the page cannot shift underneath the note being written.

  Mounted only while open (the page renders it conditionally) rather than
  toggling `open`: Svelte's prop writes do not reliably reach the Lit property,
  and unmounting closes the dialog in every case.
-->
<wa-dialog label={editing ? 'Edit note' : 'New note'} open onwa-after-hide={oncancel}>
  <div class="composer">
    {#if quote}
      <blockquote class="quote">{quote}</blockquote>
    {/if}
    <wa-textarea
      role="textbox"
      tabindex="-1"
      {placeholder}
      value={draft}
      oninput={(event: Event) => {
        draft = (event.target as HTMLTextAreaElement).value
      }}
      onkeydown={handleKeydown}
    ></wa-textarea>
    {#if showColors}
      <div class="colors" role="group" aria-label="Highlight colour">
        {#each HIGHLIGHT_COLORS as key (key)}
          <button
            type="button"
            class="swatch"
            class:selected={color === key}
            style:background={HIGHLIGHTS[key].tint}
            title={HIGHLIGHTS[key].label}
            aria-label={HIGHLIGHTS[key].label}
            aria-pressed={color === key}
            onclick={() => (color = key)}
          ></button>
        {/each}
      </div>
    {/if}
    <div class="actions">
      <ActionButton onclick={oncancel}>Cancel</ActionButton>
      <ActionButton variant="brand" onclick={submit} disabled={draft.trim()===''}>
        {editing ? 'Save changes' : 'Save note'}
      </ActionButton>
    </div>
  </div>
</wa-dialog>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    min-width: min(28rem, 80vw);
  }

  /* A selection is stored untruncated, so the preview scrolls instead of
     stretching the dialog. */
  .quote {
    max-height: 8rem;
    overflow: auto;
    margin: 0;
    padding-inline-start: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .colors {
    display: flex;
    gap: var(--wa-space-2xs);
  }

  .swatch {
    width: 1.75rem;
    height: 1.75rem;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch:hover {
    border-color: var(--wa-color-border);
  }

  .swatch:focus-visible {
    outline: 2px solid var(--wa-color-focus, var(--wa-color-brand));
    outline-offset: 2px;
  }

  .swatch.selected {
    border-color: var(--wa-color-text-default);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-2xs);
  }
</style>
