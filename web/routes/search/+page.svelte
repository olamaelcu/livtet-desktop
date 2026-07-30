<script lang="ts">
  import { deriveFacets } from "$lib/search/deriveFacets";
  import { mockHits } from "$lib/search/mock-data";
  import { filterHits } from "$lib/search/search";
  import { emptyFilters, type FilterState, type SearchHit } from "$lib/search/types";
  import CoverGrid from "$lib/search/components/cover-grid.svelte";
  import FilterChip from "$lib/search/components/filter-chip.svelte";
  import SearchBar from "$lib/search/components/search-bar.svelte";

  // The eventual swap: replace `mockHits` with an `await invoke('search_works', { ... })`
  // result, and re-derive `facets` from the response. The component tree below doesn't change.
  const allHits: readonly SearchHit[] = mockHits;
  const facets = $derived(deriveFacets(allHits));

  let rawQuery = $state("");
  let query = $state("");
  let filters: FilterState = $state(emptyFilters());

  // Debounce: typing updates `rawQuery` immediately (so the input is
  // responsive), but `query` — the value actually used for filtering
  // — only catches up after 150 ms of inactivity.
  $effect(() => {
    const next = rawQuery;
    const id = setTimeout(() => {
      query = next;
    }, 150);
    return () => clearTimeout(id);
  });

  const filteredHits = $derived(filterHits(query, filters, allHits));

  function toggleFormat(label: string) {
    const next = new Set(filters.formats);
    if (next.has(label)) next.delete(label);
    else next.add(label);
    filters = { ...filters, formats: next };
  }

  function toggleLanguage(label: string) {
    const next = new Set(filters.languages);
    if (next.has(label)) next.delete(label);
    else next.add(label);
    filters = { ...filters, languages: next };
  }
</script>

<svelte:head>
  <title>Search · livtet</title>
</svelte:head>

<main class="search-page">
  <div class="search-bar-row">
    <SearchBar bind:value={rawQuery} />
  </div>

  {#if facets.formats.length > 0}
    <section class="facet-row" aria-label="Format filter">
      <span class="facet-label">Format</span>
      {#each facets.formats as label (label)}
        <FilterChip
          {label}
          selected={filters.formats.has(label)}
          ontoggle={() => toggleFormat(label)}
        />
      {/each}
    </section>
  {/if}

  {#if facets.languages.length > 0}
    <section class="facet-row" aria-label="Language filter">
      <span class="facet-label">Language</span>
      {#each facets.languages as label (label)}
        <FilterChip
          {label}
          selected={filters.languages.has(label)}
          ontoggle={() => toggleLanguage(label)}
        />
      {/each}
    </section>
  {/if}

  {#if filteredHits.length === 0}
    <wa-callout variant="neutral">
      <wa-icon slot="icon" name="circle-info"></wa-icon>
      No matches for "{query}". Try fewer filters or a shorter query.
    </wa-callout>
  {:else}
    <CoverGrid hits={filteredHits} />
    <p class="result-count">
      {filteredHits.length}
      {filteredHits.length === 1 ? "result" : "results"}
    </p>
  {/if}
</main>

<style>
  .search-page {
    max-width: 80rem;
    margin: 0 auto;
    padding: 2rem 1.5rem 5rem;
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
  }

  .search-bar-row {
    width: 100%;
  }

  .facet-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .facet-label {
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--wa-color-text-quiet, currentColor);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-right: 0.25rem;
  }

  .result-count {
    position: fixed;
    bottom: 1rem;
    left: 50%;
    transform: translateX(-50%);
    margin: 0;
    padding: 0.5rem 1rem;
    font-size: 0.875rem;
    color: var(--wa-color-text-quiet, currentColor);
    background: var(--wa-color-surface-default, white);
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 9999px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
    z-index: 10;
    white-space: nowrap;
  }
</style>