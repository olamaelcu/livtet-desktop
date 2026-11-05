<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import ActionButton from '../../lib/components/ActionButton.svelte'
import ListLayout from '../../lib/layout/ListLayout.svelte'
import { readingListKeys } from '../../lib/query/keys'
import {
  createReadingList,
  listReadingLists,
  readingListErrorMessage,
} from '../../lib/reading-lists'

const lists = createQuery(() => ({
  queryKey: readingListKeys.lists(),
  queryFn: listReadingLists,
}))

const queryClient = useQueryClient()

let createOpen = $state(false)
let name = $state('')
let description = $state('')

const create = createMutation(() => ({
  mutationFn: () => createReadingList(name.trim(), description.trim() || null),
  onSuccess: (list) => {
    createOpen = false
    void queryClient.invalidateQueries({ queryKey: readingListKeys.lists() })
    toast.success(`Created “${list.name}”`)
  },
  onError: (error) => toast.error(readingListErrorMessage(error)),
}))

const canCreate = $derived(name.trim().length > 0 && !create.isPending)

function eventValue(event: Event): string {
  return (event.currentTarget as HTMLElement & { value: string }).value ?? ''
}

function openCreate() {
  name = ''
  description = ''
  createOpen = true
}
</script>

<ListLayout title="Reading lists">
  {#snippet action()}
    <ActionButton onclick={openCreate}>New list</ActionButton>
  {/snippet}

  {#if lists.isPending}
    <wa-spinner></wa-spinner>
  {:else if lists.isError}
    <wa-callout variant="danger">
      {readingListErrorMessage(lists.error)}
    </wa-callout>
  {:else if (lists.data ?? []).length === 0}
    <p class="empty">No reading lists yet.</p>
  {:else}
    <ul class="list-grid">
      {#each lists.data ?? [] as list (list.id)}
        <li>
          <wa-card>
            <a class="list-link" href={`/reading-lists/${list.id}`}>
              <strong>{list.name}</strong>
              {#if list.description}
                <span class="description">{list.description}</span>
              {/if}
              <span class="count">
                {list.edition_ids.length}
                {list.edition_ids.length === 1 ? 'book' : 'books'}
              </span>
            </a>
          </wa-card>
        </li>
      {/each}
    </ul>
  {/if}

  {#if createOpen}
    <wa-dialog label="New reading list" open onwa-after-hide={() => (createOpen = false)}>
      <div class="create-form">
        <wa-input
          label="Name"
          value={name}
          oninput={(event) => (name = eventValue(event))}
        ></wa-input>
        <wa-textarea
          label="Description"
          rows="3"
          value={description}
          oninput={(event) => (description = eventValue(event))}
        ></wa-textarea>
        <div class="actions">
          <ActionButton onclick={() => (createOpen = false)} disabled={create.isPending}>
            Cancel
          </ActionButton>
          <ActionButton
            variant="brand"
            onclick={() => create.mutate()}
            disabled={!canCreate}
          >
            {create.isPending ? 'Creating…' : 'Create'}
          </ActionButton>
        </div>
      </div>
    </wa-dialog>
  {/if}
</ListLayout>

<style>
  .create-form {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    min-inline-size: min(28rem, 80vw);
  }

  .create-form .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-xs);
  }

  .list-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: var(--wa-space-m);
    padding: 0;
    margin: 0;
    list-style: none;
  }

  .list-link {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    padding: var(--wa-space-s);
    color: inherit;
    text-decoration: none;
    outline: none;
  }

  .list-link:hover,
  .list-link:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .description {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-s);
    overflow-wrap: anywhere;
  }

  .count,
  .empty {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }
</style>
