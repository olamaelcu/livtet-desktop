<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { page } from '$app/state'
import ActionButton from '../../../lib/components/ActionButton.svelte'
import ListLayout from '../../../lib/layout/ListLayout.svelte'
import { readingListKeys } from '../../../lib/query/keys'
import {
  getReadingList,
  readingListErrorMessage,
  reorderReadingList,
  updateReadingList,
} from '../../../lib/reading-lists'
import { type EditionDetail, loadEditionDetail } from '../../../lib/search'

const listId = $derived(page.params.id ?? '')
const queryClient = useQueryClient()

const list = createQuery(() => ({
  queryKey: readingListKeys.list(listId),
  queryFn: () => getReadingList(listId),
  enabled: listId.length > 0,
}))

/**
 * Display metadata for the list's editions. Members cannot change from this
 * route (only their order can), so the ids are stable for the lifetime of the
 * query even as reorders update the list itself.
 */
const books = createQuery(() => ({
  queryKey: readingListKeys.books(listId),
  queryFn: async () => {
    const ids = list.data?.edition_ids ?? []
    const details = await Promise.all(
      ids.map((id) => loadEditionDetail(id).catch(() => null)),
    )
    return Object.fromEntries(ids.map((id, index) => [id, details[index]])) as Record<
      string,
      EditionDetail | null
    >
  },
  enabled: list.data !== undefined,
}))

function bookTitle(editionId: string): string {
  if (books.isPending) return 'Loading…'
  return books.data?.[editionId]?.title ?? 'Untitled book'
}

function bookAuthors(editionId: string): string {
  return (books.data?.[editionId]?.authors ?? []).map((author) => author.name).join(', ')
}

// ── Inline rename / description ──────────────────────────────────

let editing = $state(false)
let name = $state('')
let description = $state('')
let formError = $state<string | null>(null)
let saving = $state(false)

function beginEdit() {
  const current = list.data
  if (!current) return
  name = current.name
  description = current.description ?? ''
  formError = null
  editing = true
}

function cancelEdit() {
  formError = null
  editing = false
}

async function saveEdit() {
  if (saving) return
  saving = true
  formError = null
  try {
    // A blank name is rejected by the engine; sending the trimmed value keeps
    // accidental surrounding whitespace out of the stored row.
    const updated = await updateReadingList(listId, {
      name: name.trim(),
      description,
    })
    queryClient.setQueryData(readingListKeys.list(listId), updated)
    name = updated.name
    description = updated.description ?? ''
    editing = false
    void queryClient.invalidateQueries({ queryKey: readingListKeys.lists() })
    toast.success('Reading list updated')
  } catch (error) {
    formError = readingListErrorMessage(error)
  } finally {
    saving = false
  }
}

// ── Reorder ──────────────────────────────────────────────────────

/** Optimistic order while a reorder is in flight; `null` means engine order. */
let pendingOrder = $state<string[] | null>(null)
const order = $derived(pendingOrder ?? list.data?.edition_ids ?? [])
let reorderError = $state<string | null>(null)
let reordering = $state(false)

/**
 * Move one book one slot and submit the whole ordered id list. The engine
 * rejects anything that is not a permutation, so a failure restores the
 * engine's order and surfaces its message instead of pretending it applied.
 */
async function move(index: number, delta: number) {
  if (reordering) return
  const target = index + delta
  if (target < 0 || target >= order.length) return

  const next = [...order]
  const moved = next[target]
  const current = next[index]
  if (current === undefined || moved === undefined) return
  next[index] = moved
  next[target] = current

  pendingOrder = next
  reorderError = null
  reordering = true
  try {
    const updated = await reorderReadingList(listId, next)
    queryClient.setQueryData(readingListKeys.list(listId), updated)
    pendingOrder = null
  } catch (error) {
    pendingOrder = null
    reorderError = readingListErrorMessage(error)
    toast.error(reorderError)
  } finally {
    reordering = false
  }
}
</script>

<ListLayout title={list.data?.name ?? 'Reading list'}>
  {#snippet action()}
    <div class="actions">
      <wa-button href="/reading-lists">All lists</wa-button>
      <ActionButton
        onclick={beginEdit}
        disabled={editing || list.data === undefined}
      >
        Edit
      </ActionButton>
    </div>
  {/snippet}

  {#if list.isPending}
    <wa-spinner></wa-spinner>
  {:else if list.isError}
    <wa-callout variant="danger">
      {readingListErrorMessage(list.error)}
    </wa-callout>
  {:else if !list.data}
    <wa-callout variant="danger">This reading list could not be loaded.</wa-callout>
  {:else}
    {#if editing}
      <wa-card class="edit-card">
        <h2>Edit list</h2>
        <wa-input
          label="Name"
          value={name}
          oninput={(event) => (name = event.currentTarget.value)}
        ></wa-input>
        <label class="field">
          <span class="field-label">Description</span>
          <textarea
            class="textarea"
            rows="3"
            value={description}
            oninput={(event) => (description = event.currentTarget.value)}
          ></textarea>
        </label>

        {#if formError}
          <wa-callout variant="danger">{formError}</wa-callout>
        {/if}

        <div class="actions">
          <ActionButton onclick={saveEdit} variant="brand" disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </ActionButton>
          <ActionButton onclick={cancelEdit} disabled={saving}>Cancel</ActionButton>
        </div>
      </wa-card>
    {:else if list.data.description}
      <p class="description">{list.data.description}</p>
    {/if}

    {#if reorderError}
      <wa-callout variant="danger">
        {reorderError}
      </wa-callout>
    {/if}

    {#if books.isError}
      <wa-callout variant="warning">Book details could not be loaded.</wa-callout>
    {/if}

    {#if order.length === 0}
      <p class="empty">This list has no books yet.</p>
    {:else}
      <ol class="books">
        {#each order as editionId, index (editionId)}
          <li class="book">
            <span class="position">{index + 1}</span>
            <div class="book-details">
              <strong>{bookTitle(editionId)}</strong>
              {#if bookAuthors(editionId)}
                <span class="authors">{bookAuthors(editionId)}</span>
              {/if}
              <code class="edition-id">{editionId}</code>
            </div>
            <div class="move-controls">
              <ActionButton
                size="small"
                disabled={reordering || index === 0}
                onclick={() => move(index, -1)}
                aria-label={`Move ${bookTitle(editionId)} up`}
              >
                Up
              </ActionButton>
              <ActionButton
                size="small"
                disabled={reordering || index === order.length - 1}
                onclick={() => move(index, 1)}
                aria-label={`Move ${bookTitle(editionId)} down`}
              >
                Down
              </ActionButton>
            </div>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
</ListLayout>

<style>
  .actions {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .edit-card {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .edit-card h2 {
    margin: 0;
    font-size: var(--wa-font-size-m);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
  }

  .field-label {
    font-size: var(--wa-font-size-s);
  }

  .textarea {
    padding: var(--wa-space-2xs) var(--wa-space-xs);
    border: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
    border-radius: var(--wa-border-radius-m);
    background: var(--wa-color-surface-default);
    color: inherit;
    font: inherit;
    resize: vertical;
  }

  .description,
  .empty {
    color: var(--wa-color-text-secondary);
    overflow-wrap: anywhere;
  }

  .books {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    padding: 0;
    margin: 0;
    list-style: none;
  }

  .book {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s);
    border: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
    border-radius: var(--wa-border-radius-m);
  }

  .position {
    flex: none;
    min-inline-size: 2ch;
    color: var(--wa-color-text-secondary);
    text-align: end;
  }

  .book-details {
    display: flex;
    flex: 1 1;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    min-inline-size: 0;
  }

  .authors,
  .edition-id {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    overflow-wrap: anywhere;
  }

  .move-controls {
    display: flex;
    flex: none;
    gap: var(--wa-space-3xs);
  }
</style>
