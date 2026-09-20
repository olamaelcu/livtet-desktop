<script lang="ts">
import {
  type Edition,
  loadEditions,
  mapHitToEdition,
  type SearchResult,
  searchTypeahead,
} from '../../lib/search'
import BookCard from './BookCard.svelte'

let books = $state<Edition[]>([])
let query = $state('')
let offset = $state(0)
let total = $state(0)
let loading = $state(false)

const PAGE_SIZE = 20

async function fetchBooks(reset = false) {
  if (loading) return
  loading = true
  const reqOffset = reset ? 0 : offset
  try {
    const result = await loadEditions(query || undefined, reqOffset, PAGE_SIZE)
    books = reset
      ? result.hits.map(mapHitToEdition)
      : [...books, ...result.hits.map(mapHitToEdition)]
    total = result.total
    offset = books.length
  } finally {
    loading = false
  }
}

function loadMore() {
  if (books.length >= total || loading) return
  fetchBooks(false)
}

let suggestions = $state<SearchResult[]>([])
let showSuggestions = $state(false)
let debounceTimer: ReturnType<typeof setTimeout> | undefined

let openPopoverId = $state<string | null>(null)

function showPopover(id: string) {
  openPopoverId = id
}

function hidePopover() {
  openPopoverId = null
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') hidePopover()
}

$effect(() => {
  const q = query
  clearTimeout(debounceTimer)
  if (!q.trim()) {
    suggestions = []
    showSuggestions = false
    return
  }
  debounceTimer = setTimeout(async () => {
    const results = await searchTypeahead(q, 8)
    suggestions = results
    showSuggestions = results.length > 0
  }, 200)
})

function handleSearchInput(e: Event) {
  query = (e.target as HTMLInputElement).value
}
</script>

<main>
<div class="search-container" onkeydown={handleKeydown} role="textbox" tabindex={0}>
  <div class="search-wrapper">
    <wa-input
      type="text"
      placeholder="Search books..."
      value={query}
      class="search-input"
      oninput={handleSearchInput}
    ></wa-input>
    {#if showSuggestions && suggestions.length > 0}
      <div class="suggestions-dropdown" role="listbox">
        {#each suggestions as hit (hit.work_id)}
          <button
            class="suggestion-item"
            role="option"
            type="button"
            aria-selected={false}
            onclick={() => {
              query = hit.title
              showSuggestions = false
              fetchBooks(true)
            }}
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
      <div
        class="book-entry"
        role="group"
        aria-label="Book entry: {book.title}"
        onmouseenter={() => showPopover(book.id)}
        onmouseleave={hidePopover}
      >
        <BookCard
          id={book.id}
          title={book.title}
          cover_url={book.cover_url}
          popoverId="popover-{book.id}"
        />
        <wa-popover
          for="popover-{book.id}"
          placement="top"
          distance="12"
          open={openPopoverId === book.id}
          role="tooltip"
          onmouseenter={() => showPopover(book.id)}
          onmouseleave={hidePopover}
        >
          <div class="book-details">
            <h4 class="details-title">{book.title}</h4>
            <p class="details-label"><strong>Authors:</strong></p>
            {#each book.authors as author}
              <div class="details-author">
                {author.name} <wa-badge>{author.role}</wa-badge>
              </div>
            {/each}
            <p class="details-published">Published: {book.published}</p>
            <p class="details-description">{book.description}</p>
          </div>
        </wa-popover>
      </div>
    {:else}
      <div class="empty">No books? No results.</div>
    {/each}
  </div>
</wa-scroller>

{#if loading}
  <div class="loading-indicator">Loading...</div>
{/if}

{#if books.length > 0 && books.length < total}
  <div class="load-more-trigger" onclick={loadMore} role="button" tabindex="0" onkeydown={(e)=> e.key==='Enter' && loadMore()}></div>
{/if}
</main>

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
    padding: var(--wa-space-s) var(--wa-space-m);
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
    color: var(--wa-color-text-default);
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

  .book-entry {
    display: flex;
  }

  .book-entry:last-of-type {
    justify-self: flex-start;
  }

  .book-details {
    max-width: 17.5rem;
    font-size: 0.875rem;
  }

  .details-title {
    margin: 0 0 var(--wa-space-m);
  }

  .details-label {
    margin: 0 0 var(--wa-space-s);
  }

  .details-author {
    margin: 0 0 var(--wa-space-s) var(--wa-space-l);
  }

  .details-published {
    margin: 0;
  }

  .details-description {
    margin: var(--wa-space-m) 0 0;
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
