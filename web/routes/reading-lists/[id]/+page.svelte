<script lang="ts">
import { createQuery, keepPreviousData, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { goto } from '$app/navigation'
import { page } from '$app/state'
import ActionButton from '../../../lib/components/ActionButton.svelte'
import ListLayout from '../../../lib/layout/ListLayout.svelte'
import ConfirmDialog from '../../../lib/library/ConfirmDialog.svelte'
import { catalogKeys, readingListKeys, searchKeys } from '../../../lib/query/keys'
import {
  addReadingListBook,
  deleteReadingList,
  getReadingList,
  readingListErrorMessage,
  removeReadingListBook,
  reorderReadingList,
  updateReadingList,
} from '../../../lib/reading-lists'
import {
  coverUrlFor,
  type EditionDetail,
  loadEditionCovers,
  loadEditionDetail,
  type SearchResult,
  searchTypeahead,
} from '../../../lib/search'

const listId = $derived(page.params.id ?? '')
const queryClient = useQueryClient()

const list = createQuery(() => ({
  queryKey: readingListKeys.list(listId),
  queryFn: () => getReadingList(listId),
  enabled: listId.length > 0,
}))

/**
 * Display metadata for the list's editions. The key follows membership, so
 * adding or removing a book refetches details while a reorder (same set) does
 * not.
 */
const memberIds = $derived(list.data?.edition_ids ?? [])

const books = createQuery(() => ({
  queryKey: [...readingListKeys.books(listId), { ids: [...memberIds].sort() }],
  queryFn: async () => {
    const ids = memberIds
    const details = await Promise.all(ids.map((id) => loadEditionDetail(id).catch(() => null)))
    return Object.fromEntries(ids.map((id, index) => [id, details[index]])) as Record<
      string,
      EditionDetail | null
    >
  },

  enabled: list.data !== undefined,
  placeholderData: keepPreviousData,
}))

let addOpen = $state(false)
let bookQuery = $state('')
let addError = $state<string | null>(null)
let addingId = $state<string | null>(null)

const candidates = createQuery(() => ({
  queryKey: searchKeys.typeahead(bookQuery),
  queryFn: () => searchTypeahead(bookQuery, 8),
  enabled: addOpen && bookQuery.trim().length > 0,
}))

const candidateHits = $derived(
  (candidates.data ?? []).filter(
    (hit): hit is SearchResult & { edition_id: string } => hit.edition_id !== null,
  ),
)

const candidateIds = $derived(candidateHits.map((hit) => hit.edition_id))

const covers = createQuery(() => ({
  queryKey: catalogKeys.editionCovers(candidateIds),
  queryFn: () => loadEditionCovers(candidateIds),
  enabled: addOpen && candidateIds.length > 0,
}))

const coversById = $derived(
  new Map((covers.data ?? []).map((cover) => [cover.edition_id, cover.cover_path])),
)

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

function inList(editionId: string): boolean {
  return order.includes(editionId)
}

function closeAdd() {
  addOpen = false
  bookQuery = ''
  addError = null
}

async function addBook(editionId: string) {
  if (addingId !== null || reordering || inList(editionId)) return
  addingId = editionId
  addError = null
  try {
    const updated = await addReadingListBook(listId, editionId)
    queryClient.setQueryData(readingListKeys.list(listId), updated)
    pendingOrder = null
    void queryClient.invalidateQueries({ queryKey: readingListKeys.lists() })
    closeAdd()
    toast.success('Book added to this list')
  } catch (error) {
    addError = readingListErrorMessage(error)
  } finally {
    addingId = null
  }
}

let removing = $state<string | null>(null)

async function removeBook(editionId: string) {
  if (removing !== null || reordering) return
  removing = editionId
  reorderError = null
  try {
    const updated = await removeReadingListBook(listId, editionId)
    queryClient.setQueryData(readingListKeys.list(listId), updated)
    pendingOrder = null
    void queryClient.invalidateQueries({ queryKey: readingListKeys.lists() })
    toast.success('Book removed from this list')
  } catch (error) {
    toast.error(readingListErrorMessage(error))
  } finally {
    removing = null
  }
}

let deleteOpen = $state(false)
let deleting = $state(false)

async function deleteList() {
  if (deleting) return
  deleteOpen = false
  deleting = true
  try {
    await deleteReadingList(listId)
    queryClient.removeQueries({ queryKey: readingListKeys.list(listId) })
    queryClient.removeQueries({ queryKey: readingListKeys.books(listId) })
    void queryClient.invalidateQueries({ queryKey: readingListKeys.lists() })
    toast.success('Reading list deleted')
    await goto('/reading-lists')
  } catch (error) {
    toast.error(readingListErrorMessage(error))
  } finally {
    deleting = false
  }
}
</script>

<ListLayout title={list.data?.name ?? 'Reading list'}>
  {#snippet action()}
    <div class="actions">
      <wa-button href="/reading-lists">All lists</wa-button>
      <ActionButton
        onclick={() => (addOpen = true)}
        disabled={list.data === undefined || reordering || removing !== null}
      >
        Add book
      </ActionButton>
      <ActionButton
        onclick={beginEdit}
        disabled={editing || list.data === undefined}
      >
        Edit
      </ActionButton>
      <ActionButton
        onclick={() => (deleteOpen = true)}
        disabled={list.data === undefined || deleting}
      >
        Delete
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
                disabled={reordering || removing !== null || index === 0}
                onclick={() => move(index, -1)}
                aria-label={`Move ${bookTitle(editionId)} up`}
              >
                Up
              </ActionButton>
              <ActionButton
                size="small"
                disabled={reordering || removing !== null || index === order.length - 1}
                onclick={() => move(index, 1)}
                aria-label={`Move ${bookTitle(editionId)} down`}
              >
                Down
              </ActionButton>
              <ActionButton
                size="small"
                disabled={reordering || removing !== null}
                onclick={() => void removeBook(editionId)}
                aria-label={`Remove ${bookTitle(editionId)} from this list`}
              >
                Remove
              </ActionButton>
            </div>
          </li>
        {/each}
      </ol>
    {/if}
  {/if}
</ListLayout>

{#if addOpen && list.data}
  <wa-dialog label="Add a book" open onwa-after-hide={closeAdd}>
    <div class="add-book">
      <wa-input
        label="Search books"
        placeholder="Title or author"
        value={bookQuery}
        oninput={(event) => (bookQuery = event.currentTarget.value)}
      ></wa-input>

      {#if addError}
        <wa-callout variant="danger">{addError}</wa-callout>
      {/if}

      {#if bookQuery.trim().length === 0}
        <p class="empty">Type a title or author to search the library.</p>
      {:else if candidates.isPending}
        <wa-spinner></wa-spinner>
      {:else if candidates.isError}
        <wa-callout variant="danger">
          {readingListErrorMessage(candidates.error)}
        </wa-callout>
      {:else if candidateHits.length === 0}
        <p class="empty">No books match that search.</p>
      {:else}
        <ul class="candidates">
          {#each candidateHits as hit (hit.edition_id)}
            {@const added = inList(hit.edition_id)}
            {@const busy = added || addingId !== null}
            <li class="candidate-item">
              <div
                class="candidate"
                role="button"
                tabindex={busy ? -1 : 0}
                aria-disabled={busy}
                aria-label={`Add ${hit.title}`}
                onclick={() => void addBook(hit.edition_id)}
                onkeydown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault()
                    void addBook(hit.edition_id)
                  }
                }}
              >
                <figure class="candidate-cover">
                  {#if coversById.get(hit.edition_id)}
                    <img src={coverUrlFor(coversById.get(hit.edition_id))} alt="" />
                  {:else}
                    <figcaption>{hit.title}</figcaption>
                  {/if}
                </figure>
                <span class="candidate-title">{hit.title}</span>
                {#if hit.authors.length > 0}
                  <span class="candidate-authors">{hit.authors.join(', ')}</span>
                {/if}
                {#if added}
                  <wa-badge appearance="outlined">In list</wa-badge>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </wa-dialog>
{/if}

{#if deleteOpen}
  <ConfirmDialog
    open={deleteOpen}
    title="Delete reading list"
    message={`Delete “${list.data?.name ?? 'this list'}”? Its books stay in your library; only the list is removed.`}
    confirmLabel="Delete list"
    onconfirm={deleteList}
    oncancel={() => (deleteOpen = false)}
  />
{/if}

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

  .add-book {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    inline-size: 80vw;
  }

  .candidates {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(7rem, 1fr));
    gap: var(--wa-space-s);
    max-block-size: 26rem;
    overflow: auto;
    margin: 0;
    padding: var(--wa-space-3xs);
    list-style: none;
  }

  .candidate {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    inline-size: 100%;
    min-inline-size: 0;
    padding: var(--wa-space-2xs);
    border-radius: var(--wa-border-radius-m);
    color: inherit;
    text-align: start;
    cursor: pointer;
  }

  .candidate-item {
    display: flex;
    min-inline-size: 0;
  }

  .candidate:hover,
  .candidate:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .candidate[aria-disabled='true'] {
    cursor: default;
    opacity: 0.6;
  }

  .candidate-cover {
    display: grid;
    place-items: center;
    inline-size: 100%;
    aspect-ratio: 3 / 4;
    margin: 0;
    overflow: hidden;
    border: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-border-default);
    background: linear-gradient(
      135deg,
      var(--wa-color-surface-lowered) 0%,
      var(--wa-color-surface-raised) 100%
    );
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-2xs, 0.75rem);
    text-align: center;
  }

  .candidate-cover img {
    inline-size: 100%;
    block-size: 100%;
    object-fit: cover;
    display: block;
  }

  .candidate-cover figcaption {
    padding: var(--wa-space-s);
    overflow: hidden;
  }

  .candidate-title {
    display: -webkit-box;
    overflow: hidden;
    font-weight: 600;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .candidate-authors {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
