<script lang="ts">
import ActionButton from './ActionButton.svelte'

interface Props {
  /** Placeholder for the note field. */
  placeholder?: string
  /** Text selected in the publication, shown as a quoted preview. */
  quote?: string
  /** Called with the trimmed note text when the reader saves. */
  onsave: (text: string) => void
  oncancel: () => void
}

let { placeholder = 'Note text', quote, onsave, oncancel }: Props = $props()

let draft = $state('')

function submit() {
  const text = draft.trim()
  if (!text) return
  onsave(text)
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

<div class="composer">
  {#if quote}
    <blockquote class="quote">{quote}</blockquote>
  {/if}
  <wa-input
    role="textbox"
    tabindex="-1"
    {placeholder}
    value={draft}
    oninput={(event: Event) => {
      draft = (event.target as HTMLInputElement).value
    }}
    onkeydown={handleKeydown}
  ></wa-input>
  <div class="actions">
    <ActionButton variant="brand" onclick={submit} disabled={!draft.trim()}>
      Save note
    </ActionButton>
    <ActionButton onclick={oncancel}>Cancel</ActionButton>
  </div>
</div>

<style>
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    padding: var(--wa-space-s) var(--wa-space-m);
    border-top: 1px solid var(--wa-color-border);
  }

  .quote {
    margin: 0;
    padding-inline-start: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .actions {
    display: flex;
    gap: var(--wa-space-2xs);
  }
</style>
