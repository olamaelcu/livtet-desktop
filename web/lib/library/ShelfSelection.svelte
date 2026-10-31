<script lang="ts">
import { createMutation } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import { commands } from '../bindings'

export interface Props {
  editionId: string
  shelfKey?: string
  /** Title of the edition, used to reference its record when publishing. */
  bookTitle?: string
  onChange: (key: string) => void
}

let { editionId, shelfKey: shelfKeyProp = 'to-read', bookTitle = '', onChange }: Props = $props()

const shelfOptions = [
  { key: 'to-read', label: 'Want To Read', icon: 'bookmark' },
  { key: 'reading', label: 'Actively Reading', icon: 'book-open' },
  { key: 'read', label: 'Have Read', icon: 'circle-check' },
  { key: 'dnf', label: 'Did Not Finish', icon: 'ban' },
]

/** The shelf the book is on, and the icon that names it in the trigger. */
const currentShelf = $derived(
  shelfOptions.find((option) => option.key === shelfKeyProp) ?? shelfOptions[0],
)

const saveShelf = createMutation(() => {
  return {
    mutationFn: async ({ key, id }: { key: string; id: string }) => {
      if (!id) throw new Error('No edition id')
      const result = await commands.syncShelfPlacement(id, key, bookTitle)
      if (result.status === 'error')
        throw new Error(
          typeof result.error === 'string'
            ? result.error
            : result.error?.message || 'Failed to save shelf',
        )
      return result.data
    },
    onSuccess: (data, variables) => {
      // The row is local; `record_uri` says the PDS accepted the shelving
      // record too, `error` says why it did not.
      console.debug('[shelf] saved', variables, data)
      if (data?.record_uri) toast.success(`Shelved as ${variables.key}`)
      else if (data?.error) toast.warning(`Saved locally — ${data.error}`)
      else toast.success(`Shelf updated to ${variables.key}`)
      onChange(variables.key)
    },
    onError: (err) => toast.error(err instanceof Error ? err.message : 'Shelf update failed'),
  }
})

function selectShelf(key: string) {
  // Re-picking the current shelf would write (and publish) the same placement.
  if (saveShelf.isPending || key === shelfKeyProp) return
  saveShelf.mutate({ key, id: editionId ?? '' })
}

/**
 * `wa-dropdown-item` exposes `value` as a property, so the selection is read
 * from the event's item rather than an attribute (same as `LibraryToolbar`).
 */
function handleSelect(event: Event) {
  const { value } = (event as CustomEvent<{ item: HTMLElement & { value?: string } }>).detail.item
  if (value) selectShelf(value)
}

onMount(async () => {
  try {
    const result = await commands.getShelfPlacement(editionId)
    if (result.status === 'ok' && result.data) onChange(result.data)
  } catch {
    // No stored placement: the shelf the caller passed stays selected.
  }
})
</script>

<wa-dropdown onwa-select={handleSelect}>
  <wa-button
    slot="trigger"
    class="shelf-trigger"
    appearance="filled"
    with-caret
    disabled={saveShelf.isPending}
    aria-label="Shelf: {currentShelf.label}"
  >
    <wa-icon name={currentShelf.icon}></wa-icon>
    {currentShelf.label}
  </wa-button>
  {#each shelfOptions as shelf (shelf.key)}
    <wa-dropdown-item type="checkbox" checked={shelfKeyProp === shelf.key} value={shelf.key}>
      <wa-icon slot="icon" name={shelf.icon}></wa-icon>
      {shelf.label}
    </wa-dropdown-item>
  {/each}
</wa-dropdown>

<style>
/* `wa-dropdown` is `display: contents`, so the trigger button lands directly in
   the drawer's column flex and stretches to the full width. Let it size to its
   label instead, so it reads as a chip rather than a bar. */
/* `:global` because Svelte's own scope class is not applied to this custom
   element, so a scoped selector would never match. */
:global(.shelf-trigger) {
align-self: flex-start;
width: auto;
}
</style>
