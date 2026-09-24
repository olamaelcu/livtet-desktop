<script module lang="ts">
/** Anchor id for the Tag button, so the page can attach a `wa-popover` to it. */
export const TAG_BUTTON_ID = 'library-selection-tag-button'
</script>

<script lang="ts">
import ActionButton from '../components/ActionButton.svelte'

interface Props {
  count: number
  busy: boolean
  ontag: () => void
  onexport: () => void
  ondelete: () => void
  onclear: () => void
  onselectall: () => void
}

let { count, busy, ontag, onexport, ondelete, onclear, onselectall }: Props = $props()

const actionsDisabled = $derived(busy || count === 0)
</script>

<div class="bar" role="toolbar" aria-label="Selection actions">
  <span class="count">{count} selected</span>
  <ActionButton id={TAG_BUTTON_ID} onclick={ontag} disabled={actionsDisabled}>Tag</ActionButton>
  <ActionButton onclick={onexport} disabled={actionsDisabled}>Export CSV</ActionButton>
  <ActionButton onclick={ondelete} disabled={actionsDisabled}>Delete</ActionButton>
  <ActionButton onclick={onselectall} disabled={busy}>Select all matching</ActionButton>
  <ActionButton onclick={onclear} disabled={busy}>Clear</ActionButton>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) var(--wa-space-m);
  }

  .count {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-s);
  }
</style>
