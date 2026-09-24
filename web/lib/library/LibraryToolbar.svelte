<script lang="ts">
import ActionButton from '../components/ActionButton.svelte'

interface Props {
  onaddbook: () => void
  onopenfilters: () => void
  activeFilterCount?: number
  /** `id` for the Filters button, so a `wa-popover` can anchor to it via `for`. */
  filtersButtonId?: string
}

let { onaddbook, onopenfilters, activeFilterCount = 0, filtersButtonId }: Props = $props()
</script>

<div class="toolbar" role="toolbar" aria-label="Library actions">
  <wa-button-group>
    <ActionButton id={filtersButtonId} onclick={onopenfilters}>
      <wa-icon name="filter"></wa-icon>
      Filters{activeFilterCount > 0 ? ` (${activeFilterCount})` : ''}
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
