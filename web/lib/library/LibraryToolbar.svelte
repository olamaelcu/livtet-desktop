<script lang="ts">
import ActionButton from '../components/ActionButton.svelte'

interface Props {
  onaddbook: () => void
  activeFilterCount?: number
  /** Whether the filter popover is open; drives `aria-expanded`. */
  filtersExpanded?: boolean
  /** `id` for the Filters button, so a `wa-popover` can anchor to it via `for`. */
  filtersButtonId?: string
  /** Whether the library is in selection mode; drives the Select toggle. */
  selectionMode: boolean
  ontoggleselect: () => void
}

let {
  onaddbook,
  activeFilterCount = 0,
  filtersExpanded = false,
  filtersButtonId,
  selectionMode,
  ontoggleselect,
}: Props = $props()
</script>

<div class="toolbar" role="toolbar" aria-label="Library actions">
  <wa-button-group>
    <!--
      The popover's anchor toggles itself on click (WebAwesome owns `open`), so
      this button intentionally has no handler; `aria-*` advertising is threaded
      from the page.
    -->
    <ActionButton
      id={filtersButtonId}
      aria-haspopup="dialog"
      aria-expanded={filtersExpanded}
    >
      <wa-icon name="filter"></wa-icon>
      Filters{activeFilterCount > 0 ? ` (${activeFilterCount})` : ''}
    </ActionButton>
    <ActionButton onclick={ontoggleselect} aria-pressed={selectionMode}>
      <wa-icon name={selectionMode ? 'xmark' : 'check'}></wa-icon>
      {selectionMode ? 'Selecting' : 'Select'}
    </ActionButton>
    <ActionButton variant="brand" onclick={onaddbook}>
      <wa-icon name="plus"></wa-icon>
      Add book
    </ActionButton>
  </wa-button-group>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
    padding: var(--wa-space-xs) var(--wa-space-m) 0;
  }
</style>
