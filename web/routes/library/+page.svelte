<script lang="ts">
import {
  createInfiniteQuery,
  createQuery,
  keepPreviousData,
  useQueryClient,
} from '@tanstack/svelte-query'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { onDestroy, onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import { registerSearchTarget } from '../../lib/hotkeys/searchTarget.svelte'
import AddBookDrawer from '../../lib/library/AddBookDrawer.svelte'
import {
  addEditionTags,
  deleteEditions,
  exportEditionsCsv,
  matchingEditionIds,
  removeEditionTags,
} from '../../lib/library/bulk'
import ConfirmDialog from '../../lib/library/ConfirmDialog.svelte'
import {
  COVER_MIN_WIDTH,
  type CoverSize,
  loadCoverSize,
  saveCoverSize,
} from '../../lib/library/coverSize'
import EditionDetailDrawer from '../../lib/library/EditionDetailDrawer.svelte'
import FilterPanel from '../../lib/library/FilterPanel.svelte'
import { activeChips, activeFilterCount, removeAxisId } from '../../lib/library/filterAxes'
import LibraryToolbar from '../../lib/library/LibraryToolbar.svelte'
import SelectionActionBar, { TAG_BUTTON_ID } from '../../lib/library/SelectionActionBar.svelte'
import { Selection } from '../../lib/library/selection.svelte'
import { sentinel } from '../../lib/library/sentinel'
import TagPicker from '../../lib/library/TagPicker.svelte'
import { catalogKeys, searchKeys } from '../../lib/query/keys'
import { openReaderWindow } from '../../lib/reader/read'
import {
  coverUrlFor,
  type Edition,
  type EditionFilters,
  loadEditionCovers,
  loadEditions,
  loadFilterOptions,
  mapHitToEdition,
  searchTypeahead,
} from '../../lib/search'
import BookCard from './BookCard.svelte'
import LibraryLayout from './LibraryLayout.svelte'

const PAGE_SIZE = 20
const TYPEAHEAD_LIMIT = 8
const DEBOUNCE_MS = 200
const FILTERS_BUTTON_ID = 'library-filters-button'

const queryClient = useQueryClient()

let queryInput = $state('')
let query = $state('')
let dismissedQuery = $state<string | null>(null)
let debounceTimer: ReturnType<typeof setTimeout> | undefined
let filters = $state<EditionFilters>({})
let filtersOpen = $state(false)
let coverSize = $state<CoverSize>(loadCoverSize())

function setCoverSize(size: CoverSize) {
  coverSize = size
  saveCoverSize(size)
}

onDestroy(() => clearTimeout(debounceTimer))

const editions = createInfiniteQuery(() => ({
  queryKey: searchKeys.editions(query, filters),
  queryFn: ({ pageParam }) => loadEditions(query || undefined, filters, pageParam, PAGE_SIZE),
  initialPageParam: 0,
  getNextPageParam: (lastPage, allPages) => {
    if (lastPage.hits.length === 0) return undefined
    const loaded = allPages.reduce((count, page) => count + page.hits.length, 0)
    return loaded < lastPage.total ? loaded : undefined
  },
}))

const books = $derived(
  (editions.data?.pages ?? []).flatMap((page) => page.hits.map(mapHitToEdition)).filter(isEdition),
)

function isEdition(book: Edition): book is Edition & { edition_id: string } {
  return book.kind === 'edition' && book.edition_id !== null
}

const editionIds = $derived(books.map((book) => book.edition_id))

const covers = createQuery(() => ({
  queryKey: catalogKeys.editionCovers(editionIds),
  queryFn: () => loadEditionCovers(editionIds),
  enabled: editionIds.length > 0,
  placeholderData: keepPreviousData,
}))

const coversById = $derived(
  new Map((covers.data ?? []).map((cover) => [cover.edition_id, cover.cover_path])),
)

const typeahead = createQuery(() => ({
  queryKey: searchKeys.typeahead(query),
  queryFn: () => searchTypeahead(query, TYPEAHEAD_LIMIT),
  enabled: query.trim().length > 0,
}))

const filterOptions = createQuery(() => ({
  queryKey: searchKeys.filterOptions(),
  queryFn: loadFilterOptions,
  staleTime: 5 * 60 * 1000,
}))

const chips = $derived(activeChips(filters, filterOptions.data))
const activeCount = $derived(activeFilterCount(filters))

const totalCount = $derived(editions.data?.pages[0]?.total ?? null)

const suggestions = $derived(typeahead.data ?? [])
const showSuggestions = $derived(
  query.trim().length > 0 && suggestions.length > 0 && dismissedQuery !== query,
)

let addBookOpen = $state(false)
let selectedEditionId = $state<string | null>(null)
let detailOpen = $state(false)

function openDetail(editionId: string) {
  selectedEditionId = editionId
  detailOpen = true
}

async function openBook(editionId: string, hasFile: boolean) {
  if (selection.mode || !hasFile) return
  try {
    await openReaderWindow(editionId)
  } catch (error) {
    toast.error(messageOf(error))
  }
}

const selection = new Selection()
let busy = $state(false)
let tagOpen = $state(false)
let confirmDelete = $state(false)
let selectAllIds = $state<string[]>([])

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return 'Something went wrong'
}

async function runDelete() {
  busy = true
  try {
    const outcome = await deleteEditions([...selection.selected])
    await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    selection.clear()
    toast.success(`Deleted ${outcome.deleted}`)
    confirmDelete = false
  } catch (error) {
    toast.error(messageOf(error))
  } finally {
    busy = false
  }
}

async function runExport() {
  try {
    const path = await saveDialog({
      defaultPath: 'library-export.csv',
      filters: [{ name: 'CSV', extensions: ['csv'] }],
    })
    if (!path) return
    busy = true
    const outcome = await exportEditionsCsv([...selection.selected], path)
    toast.success(`Exported ${outcome.rows} editions`)
  } catch (error) {
    toast.error(messageOf(error))
  } finally {
    busy = false
  }
}

async function runAddTag(name: string) {
  busy = true
  try {
    const outcome = await addEditionTags([...selection.selected], name)
    await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    await queryClient.invalidateQueries({ queryKey: searchKeys.filterOptions() })
    toast.success(`Added "${outcome.tag.label}" to ${outcome.changed} editions`)
  } catch (error) {
    toast.error(messageOf(error))
  } finally {
    busy = false
  }
}

async function runRemoveTag(tagId: string) {
  busy = true
  try {
    const outcome = await removeEditionTags([...selection.selected], tagId)
    await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    await queryClient.invalidateQueries({ queryKey: searchKeys.filterOptions() })
    toast.success(`Removed "${outcome.tag.label}" from ${outcome.changed} editions`)
  } catch (error) {
    toast.error(messageOf(error))
  } finally {
    busy = false
  }
}

async function runSelectAll() {
  busy = true
  try {
    selectAllIds = await matchingEditionIds(query || undefined, filters)
    selection.selectAll(selectAllIds)
    toast.success(`Selected ${selectAllIds.length} matching editions`)
  } catch (error) {
    toast.error(messageOf(error))
  } finally {
    busy = false
  }
}

function loadMore() {
  if (editions.hasNextPage && !editions.isFetchingNextPage) editions.fetchNextPage()
}

let scrollElement = $state<HTMLElement>()
let searchInput = $state<HTMLElement>()

onMount(() => registerSearchTarget(() => searchInput?.focus()))

function handleSearchInput(event: Event) {
  const value = (event.target as HTMLInputElement).value
  queryInput = value
  clearTimeout(debounceTimer)
  debounceTimer = setTimeout(() => {
    query = value
  }, DEBOUNCE_MS)
}

function selectSuggestion(title: string) {
  clearTimeout(debounceTimer)
  queryInput = title
  query = title
  dismissedQuery = title
}
</script>

{#snippet selectionDock()}
  <SelectionActionBar
    count={selection.count}
    {busy}
    ontag={() => (tagOpen = !tagOpen)}
    onexport={runExport}
    ondelete={() => (confirmDelete = true)}
    onclear={() => selection.clear()}
    onselectall={runSelectAll}
  />
{/snippet}

<LibraryLayout bind:scrollElement dock={selection.mode ? selectionDock : undefined}>
  {#snippet toolbar()}
<LibraryToolbar
  onaddbook={() => (addBookOpen = true)}
  activeFilterCount={activeCount}
  filtersExpanded={filtersOpen}
  filtersButtonId={FILTERS_BUTTON_ID}
  selectionMode={selection.mode}
  ontoggleselect={() => (selection.mode = !selection.mode)}
  {coverSize}
  oncoversizechange={setCoverSize}
/>
  {/snippet}
  {#snippet strip()}
{#if chips.length > 0}
  <div class="filter-chips" role="group" aria-label="Active filters">
    {#each chips as chip (chip.key)}
      <button
        type="button"
        class="filter-chip"
        aria-label={`Remove ${chip.label} filter`}
        onclick={() => (filters = removeAxisId(filters, chip.axis, chip.id))}
      >
        {chip.label}
        <wa-icon name="xmark" aria-hidden="true"></wa-icon>
      </button>
    {/each}
    <button type="button" class="filter-chip clear" onclick={() => (filters = {})}>
      Clear filters
    </button>
  </div>
{/if}
<div class="search-container">
  <div class="search-wrapper">
    <wa-input
      id="library-search"
      bind:this={searchInput}
      type="text"
      placeholder="Search books..."
      value={queryInput}
      class="search-input"
      oninput={handleSearchInput}
    ></wa-input>
    {#if showSuggestions}
      <div class="suggestions-dropdown" role="listbox">
        {#each suggestions as hit (hit.work_id)}
          <button
            class="suggestion-item"
            role="option"
            type="button"
            aria-selected={false}
            onclick={() => selectSuggestion(hit.title)}
          >
            <span class="suggestion-title">{hit.title}</span>
            {#if hit.authors?.length}
              <span class="suggestion-authors">{hit.authors.join(', ')}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>
{#if !editions.isPending && !editions.isError && totalCount !== null}
  <div class="result-count" role="status" aria-live="polite">
    {#if editions.hasNextPage}
      {books.length} of {totalCount} {totalCount === 1 ? 'result' : 'results'}
    {:else}
      {totalCount} {totalCount === 1 ? 'result' : 'results'}
    {/if}
  </div>
{/if}
  {/snippet}
  <div
    class="book-list"
    class:selection-docked={selection.mode}
    style="--col-min: {COVER_MIN_WIDTH[coverSize]}"
  >
    {#each books as book (book.id)}
      <BookCard
        title={book.title}
        cover_url={coverUrlFor(coversById.get(book.edition_id))}
        selectable={selection.mode}
        selected={selection.selected.has(book.edition_id)}
        in_filesystem={book.has_file}
        onclick={() =>
          selection.mode ? selection.toggle(book.edition_id) : openDetail(book.edition_id)}
        ondblclick={() => void openBook(book.edition_id, book.has_file)}
      />
    {:else}
      {#if !editions.isPending && !editions.isError}
        <div class="empty">No books? No results.</div>
      {/if}
    {/each}
  </div>
  {#if editions.hasNextPage}
    <div class="load-more" use:sentinel={{ root: scrollElement, onvisible: loadMore }}>
      <button
        type="button"
        class="load-more-button"
        onclick={loadMore}
        disabled={editions.isFetchingNextPage}
      >
        {#if editions.isFetchingNextPage}
          Loading…
        {:else}
          Load more ({books.length} of {totalCount})
        {/if}
      </button>
    </div>
  {/if}
  {#snippet overlays()}
{#if selection.mode}
<wa-popover
    for={TAG_BUTTON_ID}
    label="Tags"
    placement="top-start"
    open={tagOpen}
    onwa-after-show={() => (tagOpen = true)}
    onwa-after-hide={() => (tagOpen = false)}
  >
    <TagPicker onadd={runAddTag} onremove={runRemoveTag} onclose={() => (tagOpen = false)} />
  </wa-popover>
{/if}
<wa-popover
  for={FILTERS_BUTTON_ID}
  label="Filters"
  placement="bottom-start"
  open={filtersOpen}
  onwa-after-show={() => (filtersOpen = true)}
  onwa-after-hide={() => (filtersOpen = false)}
>
  <FilterPanel
    {filters}
    onchange={(next) => (filters = next)}
    onclose={() => (filtersOpen = false)}
  />
</wa-popover>


{#if editions.isPending}
  <div class="loading-indicator">Loading...</div>
{:else if editions.isError}
  <div class="error-indicator">
    <p>Could not load books.</p>
    <button type="button" class="retry" onclick={() => editions.refetch()}>Retry</button>
  </div>
{/if}

<ConfirmDialog
  open={confirmDelete}
  title="Delete editions"
  message={`Delete ${selection.count} selected ${selection.count === 1 ? 'edition' : 'editions'}? This cannot be undone.`}
  confirmLabel="Delete"
  onconfirm={runDelete}
  oncancel={() => (confirmDelete = false)}
/>

<AddBookDrawer open={addBookOpen} onclose={() => (addBookOpen = false)} />
<EditionDetailDrawer
  editionId={selectedEditionId}
  open={detailOpen}
  onclose={() => (detailOpen = false)}
/>
  {/snippet}
</LibraryLayout>

<style>
  .search-container {
    padding: var(--wa-space-xs);
  }

  .search-wrapper {
    position: relative;
  }

  .search-input {
    width: 100%;
    padding: 0 var(--wa-space-s);
    font-size: 1rem;
    border-radius: var(--wa-radius-m);
    border: 1px solid var(--wa-color-border);
  }

  .result-count {
    padding: 0 var(--wa-space-m);
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-secondary);
  }

  .suggestions-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    margin-top: var(--wa-space-3xs);
    background: var(--wa-color-surface-default);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    box-shadow: var(--wa-shadow-l);
    z-index: 100;
    max-height: 16rem;
    overflow-y: auto;
  }

  .suggestion-item {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    width: 100%;
    padding: var(--wa-space-m);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    color: var(--wa-color-text-default);
    align-items: start;
  }

  .suggestion-item:hover {
    background: var(--wa-color-surface-alt);
  }

  .suggestion-title {
    font-size: var(--wa-font-size-s);
    font-weight: 500;
  }

  .suggestion-authors {
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-secondary);
  }

  .book-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--col-min, 8rem), 1fr));
    gap: var(--wa-space-xs);
    padding: var(--wa-space-s) var(--wa-space-l);

    & > .empty {
      grid-column: 1 / -1;
      padding: var(--wa-space-xl);
      text-align: center;
    }
  }

  .book-list.selection-docked {
    padding-bottom: calc(var(--wa-space-l) * 2.5);
  }

  .loading-indicator {
    padding: var(--wa-space-m);
    text-align: center;
  }

  .filter-chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--wa-space-2xs);
    padding: 0 var(--wa-space-m);
  }

  .filter-chip {
    display: inline-flex;
    align-items: center;
    gap: var(--wa-space-3xs);
    padding: var(--wa-space-3xs) var(--wa-space-2xs);
    font-size: var(--wa-font-size-2xs);
    color: var(--wa-color-text-normal);
    background: var(--wa-color-surface-alt);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    cursor: pointer;
  }

  .filter-chip:hover {
    background: var(--wa-color-surface-default);
  }

  .filter-chip.clear {
    font-weight: 500;
  }

  .error-indicator {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    align-items: center;
    padding: var(--wa-space-m);
    text-align: center;
    color: var(--wa-color-danger);
  }

  .error-indicator p {
    margin: 0;
  }

  .retry {
    padding: var(--wa-space-xs) var(--wa-space-s);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-default);
    cursor: pointer;
  }

  .load-more {
    display: flex;
    justify-content: center;
    padding: var(--wa-space-m);
  }

  .load-more-button {
    padding: var(--wa-space-xs) var(--wa-space-m);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-default);
    cursor: pointer;
  }

  .load-more-button:disabled {
    cursor: wait;
  }
</style>
