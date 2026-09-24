<script lang="ts">
import { createInfiniteQuery, createQuery } from '@tanstack/svelte-query'
import { onDestroy } from 'svelte'
import AddBookDrawer from '../../lib/library/AddBookDrawer.svelte'
import EditionDetailDrawer from '../../lib/library/EditionDetailDrawer.svelte'
import LibraryToolbar from '../../lib/library/LibraryToolbar.svelte'
import { searchKeys } from '../../lib/query/keys'
import { type Edition, loadEditions, mapHitToEdition, searchTypeahead } from '../../lib/search'
import BookCard from './BookCard.svelte'

const PAGE_SIZE = 20
const TYPEAHEAD_LIMIT = 8
const DEBOUNCE_MS = 200

let queryInput = $state('')
let query = $state('')
let dismissedQuery = $state<string | null>(null)
let debounceTimer: ReturnType<typeof setTimeout> | undefined

onDestroy(() => clearTimeout(debounceTimer))

const editions = createInfiniteQuery(() => ({
  queryKey: searchKeys.editions(query),
  queryFn: ({ pageParam }) => loadEditions(query || undefined, pageParam, PAGE_SIZE),
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

const typeahead = createQuery(() => ({
  queryKey: searchKeys.typeahead(query),
  queryFn: () => searchTypeahead(query, TYPEAHEAD_LIMIT),
  enabled: query.trim().length > 0,
}))

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

function loadMore() {
  if (editions.hasNextPage && !editions.isFetchingNextPage) editions.fetchNextPage()
}

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

<main>
<LibraryToolbar onaddbook={() => (addBookOpen = true)} />
<div class="search-container">
  <div class="search-wrapper">
    <wa-input
      id="library-search"
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

<wa-scroller orientation="vertical" class="book-scroller" onscrollend={loadMore}>
  <div class="book-list">
    {#each books as book (book.id)}
      <BookCard
        title={book.title}
        cover_url={book.cover_url}
        onclick={() => openDetail(book.edition_id)}
      />
    {:else}
      <div class="empty">No books? No results.</div>
    {/each}
  </div>
</wa-scroller>

{#if editions.isPending}
  <div class="loading-indicator">Loading...</div>
{/if}

{#if editions.hasNextPage}
  <div class="load-more-trigger" onclick={loadMore} role="button" tabindex="0" onkeydown={(e)=> e.key==='Enter' && loadMore()}></div>
{/if}
</main>

<AddBookDrawer open={addBookOpen} onclose={() => (addBookOpen = false)} />
<EditionDetailDrawer
  editionId={selectedEditionId}
  open={detailOpen}
  onclose={() => (detailOpen = false)}
/>

<style>
  main {
    display: flex;
    width: 100%;
    height: 100%;
    flex-direction: column;
    padding: 0;
    margin: 0;
  }

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

  .book-scroller {
    flex: 1 1;
    max-width: 100%;
    align-items: start;
    padding: 0 var(--wa-space-l);
  }

  .book-list {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    gap: var(--wa-space-s);
    justify-content: center;
  
    & > .empty {
      height: 100%;
    }
  }

  .loading-indicator {
    padding: var(--wa-space-m);
    text-align: center;
  }

  .load-more-trigger {
    height: 1px;
    width: 100%;
  }
</style>
