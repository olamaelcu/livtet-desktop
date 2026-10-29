<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys, shelfKeys } from '../query/keys'
import { importShelvedBook, listShelvedBooks, SHELF_ORDER, type ShelvedBook } from '../shelves'

interface Props {
  open: boolean
  onclose: () => void
}

let { open, onclose }: Props = $props()

/** The account can shelve thousands; render a page of matches, not all of them. */
const VISIBLE_LIMIT = 100

const queryClient = useQueryClient()

// Reading the whole repository takes a few PDS round trips, so it happens once
// per open and stays cached until the drawer is dismissed.
const shelving = createQuery(() => ({
  queryKey: shelfKeys.shelvedBooks(),
  queryFn: listShelvedBooks,
  enabled: open,
}))

let searchInput = $state('')
let shelfFilter = $state('all')
let busyUri = $state<string | null>(null)
/** `book_uri` → what the import did, so a row cannot be added twice by accident. */
let added = $state<Record<string, 'imported' | 'duplicate'>>({})

const rows = $derived(shelving.data ?? [])

/** Shelves that actually hold books, in library order. */
const shelfOptions = $derived.by(() =>
  SHELF_ORDER.map((key) => ({
    key,
    label: rows.find((row) => row.shelf_key === key)?.shelf_name ?? key,
    count: rows.filter((row) => row.shelf_key === key).length,
  })).filter((option) => option.count > 0),
)

const matches = $derived.by(() => {
  const query = searchInput.trim().toLowerCase()
  return rows.filter((row) => {
    if (shelfFilter !== 'all' && row.shelf_key !== shelfFilter) return false
    return query.length === 0 || row.title.toLowerCase().includes(query)
  })
})

const visible = $derived(matches.slice(0, VISIBLE_LIMIT))

function eventValue(event: Event): string {
  return (event.currentTarget as HTMLElement & { value: string }).value ?? ''
}

async function add(book: ShelvedBook) {
  if (busyUri !== null) return
  busyUri = book.book_uri
  try {
    const outcome = await importShelvedBook(book)
    added = { ...added, [book.book_uri]: outcome.duplicate ? 'duplicate' : 'imported' }
    await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    if (outcome.duplicate) {
      toast.info(`“${outcome.title}” is already in your library`)
    } else {
      toast.success(
        book.shelf_name
          ? `Added “${outcome.title}” from ${book.shelf_name}`
          : `Added “${outcome.title}”`,
      )
    }
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not import that book')
  } finally {
    busyUri = null
  }
}
</script>

<wa-drawer
  label="Add from your shelves"
  placement="end"
  open={open}
  onwa-after-hide={onclose}
>
  <div class="panel">
    <p class="summary">
      Every book shelved in your ATProto account, across all of your shelves. Adding one as a
      virtual book keeps the shelf it sits on as its reading status.
    </p>

    <wa-input
      label="Search"
      placeholder="Title…"
      value={searchInput}
      oninput={(event: Event) => (searchInput = eventValue(event))}
    ></wa-input>

    <wa-select
      label="Shelf"
      value={shelfFilter}
      onchange={(event: Event) => (shelfFilter = eventValue(event))}
    >
      <wa-option value="all">All shelves ({rows.length})</wa-option>
      {#each shelfOptions as option (option.key)}
        <wa-option value={option.key}>{option.label} ({option.count})</wa-option>
      {/each}
    </wa-select>

    {#if shelving.isPending}
      <p class="muted">Reading your shelves…</p>
    {:else if shelving.isError}
      <wa-callout variant="danger">
        {shelving.error instanceof Error ? shelving.error.message : 'Could not read your shelves'}
      </wa-callout>
      <ActionButton onclick={() => shelving.refetch()}>Try again</ActionButton>
    {:else if rows.length === 0}
      <p class="muted">This account has no shelved books yet.</p>
    {:else if matches.length === 0}
      <p class="muted">No shelved titles match that search.</p>
    {:else}
      <p class="muted small">Showing {visible.length} of {matches.length} {matches.length === 1 ? 'book' : 'books'}</p>
      <ul class="results">
        {#each visible as book (book.book_uri)}
          <li class="result">
            <div class="result-body">
              <span class="result-title">{book.title}</span>
              {#if book.shelf_name}
                <wa-badge appearance="outlined">{book.shelf_name}</wa-badge>
              {/if}
            </div>
            {#if added[book.book_uri]}
              <span class="muted small">
                {added[book.book_uri] === 'duplicate' ? 'Already added' : 'Added'}
              </span>
            {:else}
              <ActionButton disabled={busyUri !== null} onclick={() => void add(book)}>
                {busyUri === book.book_uri ? 'Adding…' : 'Add'}
              </ActionButton>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</wa-drawer>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
    padding-top: var(--wa-space-m);
  }

  .summary {
    margin: 0;
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .results {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs);
    margin: 0;
    padding: 0;
    max-height: 32rem;
    overflow-y: auto;
    list-style: none;
  }

  .result {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .result:first-of-type {
    border-top: none;
  }

  .result-body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    gap: 0.125rem;
  }

  .result-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: var(--wa-font-weight-semibold);
  }

  .small {
    font-size: var(--wa-font-size-xs);
  }

  .muted {
    color: var(--wa-color-text-secondary);
  }
</style>
