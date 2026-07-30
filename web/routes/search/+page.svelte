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

<wa-page>
  <header slot="header">
    <SearchBar bind:value={rawQuery} />
  </header>

  <div slot="main-header">
    {#if facets.formats.length > 0}
      <section class="facet-row" aria-label="Format filter">
        <span class="facet-label">Format</span>
        {#if filters.formats.size > 0}
          <wa-badge variant="brand" appearance="filled">
            {filters.formats.size}
          </wa-badge>
        {/if}
        {#each facets.formats as label (label)}
          <FilterChip
            id={`format-${label}`}
            {label}
            selected={filters.formats.has(label)}
            ontoggle={() => toggleFormat(label)}
          />
          <wa-tooltip for={`format-${label}`} content={`Show only ${label} books`}></wa-tooltip>
        {/each}
      </section>
    {/if}

    {#if facets.languages.length > 0}
      <section class="facet-row" aria-label="Language filter">
        <span class="facet-label">Language</span>
        {#if filters.languages.size > 0}
          <wa-badge variant="brand" appearance="filled">
            {filters.languages.size}
          </wa-badge>
        {/if}
        {#each facets.languages as label (label)}
          <FilterChip
            id={`language-${label}`}
            {label}
            selected={filters.languages.has(label)}
            ontoggle={() => toggleLanguage(label)}
          />
          <wa-tooltip for={`language-${label}`} content={`Show only ${label} books`}></wa-tooltip>
        {/each}
      </section>
    {/if}
  </div>

  <main>
    <wa-scroller orientation="vertical" class="result-scroller">
      {#if filteredHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          No matches for "{query}". Try fewer filters or a shorter query.
        </wa-callout>
      {:else}
        <CoverGrid hits={filteredHits} />
      {/if}
    </wa-scroller>
  </main>

  <footer slot="main-footer">
    <p class="result-count">
      {filteredHits.length}
      {filteredHits.length === 1 ? "result" : "results"}
    </p>
  </footer>
</wa-page>

<style>
.facet-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    flex-basis: 100%;
  }

  .facet-label {
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--wa-color-text-quiet, currentColor);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-right: 0.25rem;
  }

  /* Scroller needs a max-height to actually scroll. Subtracts room for
     header + main-header + footer; tune if those grow. */
  .result-scroller {
    max-height: calc(100vh - 14rem);
  }

  /* Pin the result count to the bottom-center of the viewport.
     Semantically it lives in <wa-page>'s main-footer slot, but the slot
     itself is collapsed via ::part(main-footer) { display: contents }
     below so the floating element doesn't push the grid up. */
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

  main, header {
    padding: 0;
    margin: 0;
  }
  
  div[slot=main-header] {
    padding: var(--wa-space-m);
    margin: 0;
  }

</style>
