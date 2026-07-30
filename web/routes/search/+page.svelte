<script lang="ts">
  import { deriveFacets } from "$lib/search/deriveFacets";
  import { filterHits } from "$lib/search/search";
  import { emptyFilters, type FilterState } from "$lib/search/types";
  import { commands, type SearchHitRow } from "$lib/bindings";
  import SearchView from "$lib/search/components/search-view.svelte";
  import FilterChip from "$lib/search/components/filter-chip.svelte";
  import CoverGrid from "$lib/search/components/cover-grid.svelte";
  import CommandScope from "$lib/commands/components/command-scope.svelte";

  const SEARCH_LIMIT = 50;

  // Renamed from `filters` to `filterState` to avoid shadowing the
  // `filters` snippet name in SearchView's child scope.
  let filterState: FilterState = $state(emptyFilters());

  function toggleFormat(label: string) {
    const next = new Set(filterState.formats);
    if (next.has(label)) next.delete(label);
    else next.add(label);
    filterState = { ...filterState, formats: next };
  }

  function toggleLanguage(label: string) {
    const next = new Set(filterState.languages);
    if (next.has(label)) next.delete(label);
    else next.add(label);
    filterState = { ...filterState, languages: next };
  }

  // Extracted from the inline prop so the reference is stable across renders.
  // An inline arrow on the consumer side would create a new closure each
  // render; SearchView's search effect reads `runSearch`, so a fresh
  // reference each render triggers reactive re-runs that feed back into its
  // `allHits = []` reset path and trip effect_update_depth_exceeded.
  async function runSearch(
    q: string,
    limit: number,
    onResults: (hits: SearchHitRow[]) => void,
    onError?: (err: string) => void,
  ) {
    await commands
      .search(q, limit)
      .then((r) => {
        if (r.status === "ok") onResults(r.data);
        else {
          onError?.(r.error);
          onResults([]);
        }
      })
      .catch((e) => {
        onError?.(String(e));
        onResults([]);
      });
  }
</script>

<CommandScope id="search">
  <SearchView
    title="Search · livtet"
    limit={SEARCH_LIMIT}
    {runSearch}
    prompt="Type a query to search the catalog."
    noResults="No matches."
  >
    {#snippet filters({ hits })}
      {@const facets = deriveFacets(hits)}
      {#if facets.formats.length > 0}
        <section class="facet-row" aria-label="Format filter">
          <span class="facet-label">Format</span>
          {#if filterState.formats.size > 0}
            <wa-badge variant="brand" appearance="filled">
              {filterState.formats.size}
            </wa-badge>
          {/if}
          {#each facets.formats as label (label)}
            <FilterChip
              id={`format-${label}`}
              {label}
              selected={filterState.formats.has(label)}
              ontoggle={() => toggleFormat(label)}
            />
          {/each}
        </section>
      {/if}

      {#if facets.languages.length > 0}
        <section class="facet-row" aria-label="Language filter">
          <span class="facet-label">Language</span>
          {#if filterState.languages.size > 0}
            <wa-badge variant="brand" appearance="filled">
              {filterState.languages.size}
            </wa-badge>
          {/if}
          {#each facets.languages as label (label)}
            <FilterChip
              id={`language-${label}`}
              {label}
              selected={filterState.languages.has(label)}
              ontoggle={() => toggleLanguage(label)}
            />
          {/each}
        </section>
      {/if}
    {/snippet}

    {#snippet result({ hits, query })}
      {@const filtered = filterHits(query, filterState, hits)}
      {#if filtered.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          No matches for "{query}". Try fewer filters or a shorter query.
        </wa-callout>
      {:else}
        <CoverGrid hits={filtered} />
      {/if}
    {/snippet}

    {#snippet footer({ count })}
      <p class="result-count">
        {count}
        {count === 1 ? "result" : "results"}
      </p>
    {/snippet}
  </SearchView>
</CommandScope>

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

  /* Pin the result count to the bottom-center of the viewport.
     Semantically it lives in <wa-page>'s main-footer slot, but the slot
     itself is collapsed via ::part(main-footer) { display: contents }
     in app.css so the floating element doesn't push the grid up. */
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
