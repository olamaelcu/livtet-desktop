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
  { key: 'to-read', label: 'To-Read' },
  { key: 'dnf', label: 'Did Not Finish' },
  { key: 'reading', label: 'Reading' },
]

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
  if (saveShelf.isPending) return
  saveShelf.mutate({ key, id: editionId ?? '' })
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

<div class="shelf">
  {#each shelfOptions as s}
    <button
      class="shelf-btn"
      class:selected={shelfKeyProp === s.key}
      onclick={() => selectShelf(s.key)}
      type="button"
      disabled={saveShelf.isPending}
    >
      {s.label}
    </button>
  {/each}
</div>

<style>
  .shelf { display: flex; gap: var(--wa-space-xs); flex-wrap: wrap; }
  .shelf-btn { flex: 1 1 auto; min-width: 6rem; }
  .shelf-btn.selected { border-color: var(--wa-color-brand); background: var(--wa-color-brand); color: var(--wa-color-brand-contrast); }
</style>
