<script lang="ts">
import ActionButton from '../components/ActionButton.svelte'
import {
  COVER_SIZES,
  type CoverSize,
  coverSizeFromIndex,
  coverSizeIndex,
  coverSizeLabel,
} from './coverSize'

interface Props {
  onaddbook: () => void
  /** Opens the virtual-book drawer (catalog-only book, no file). */
  onaddvirtualbook: () => void
  /** Opens the shelved-books drawer (the account's ATProto shelving). */
  onshelvedbooks: () => void
  /** Whether an ATProto account is signed in; the shelf entry hides otherwise. */
  atprotoSignedIn: boolean
  activeFilterCount?: number
  /** Whether the filter popover is open; drives `aria-expanded`. */
  filtersExpanded?: boolean
  /** `id` for the Filters button, so a `wa-popover` can anchor to it via `for`. */
  filtersButtonId?: string
  /** Whether the library is in selection mode; drives the Select toggle. */
  selectionMode: boolean
  ontoggleselect: () => void
  /** Current cover size; drives the size slider. */
  coverSize: CoverSize
  oncoversizechange: (size: CoverSize) => void
}

let {
  onaddbook,
  onaddvirtualbook,
  onshelvedbooks,
  atprotoSignedIn,
  activeFilterCount = 0,
  filtersExpanded = false,
  filtersButtonId,
  selectionMode,
  ontoggleselect,
  coverSize,
  oncoversizechange,
}: Props = $props()

function coverSizeFormatter(node: Element) {
  const slider = node as HTMLElement & { valueFormatter?: (value: number) => string }
  customElements.whenDefined('wa-slider').then(() => {
    slider.valueFormatter = (value) => coverSizeLabel(coverSizeFromIndex(value))
  })
}

function handleCoverSize(event: Event) {
  const value = Number((event.currentTarget as HTMLElement & { value: number }).value)
  oncoversizechange(coverSizeFromIndex(value))
}
function handleSelect(event: CustomEvent) {
  // `wa-dropdown-item` exposes `value` as a property; WebAwesome never renders
  // the attribute, so `getAttribute('value')` reads null here.
  const { value } = event.detail.item as HTMLElement & { value?: string }
  if (value === 'add-book') {
    onaddbook()
  } else if (value === 'add-virtual') {
    onaddvirtualbook()
  } else if (value === 'add-shelved') {
    onshelvedbooks()
  }
}
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
    <wa-dropdown onwa-select={handleSelect}>
      <wa-button slot="trigger" appearance="filled" variant="brand" with-caret>
        <wa-icon name="plus"></wa-icon>
        Add
      </wa-button>
      <wa-dropdown-item value="add-book">
        <wa-icon slot="icon" name="plus"></wa-icon>
        Add book
      </wa-dropdown-item>
      <wa-dropdown-item value="add-virtual">
        <wa-icon slot="icon" name="book"></wa-icon>
        Add virtual book
      </wa-dropdown-item>
      {#if atprotoSignedIn}
        <wa-dropdown-item value="add-shelved">
          <wa-icon slot="icon" name="layer-group"></wa-icon>
          Add from your shelves
        </wa-dropdown-item>
      {/if}
    </wa-dropdown>
  </wa-button-group>
  <wa-slider
    class="cover-slider"
    {@attach coverSizeFormatter}
    label="Cover size: {coverSizeLabel(coverSize)}"
    min="0"
    max={COVER_SIZES.length - 1}
    step="1"
    value={coverSizeIndex(coverSize)}
    with-markers
    with-tooltip
    oninput={handleCoverSize}
  ></wa-slider>
</div>

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--wa-space-xs);
    padding: var(--wa-space-xs) var(--wa-space-m);
  }

  .cover-slider {
    width: 12rem;
    margin-left: auto;
  }
</style>
