<script lang="ts">
  import type { SearchHitRow } from "$lib/bindings";
  import CoverGrid from "$lib/search/components/cover-grid.svelte";
  import SearchBar from "$lib/search/components/search-bar.svelte";
  import { runSearch } from "$lib/remote/chain";

  const LIMIT = 50;

  let allHits = $state<SearchHitRow[]>([]);
  let rawQuery = $state("");
  let debouncedQuery = $state("");

  $effect(() => {
    const next = rawQuery;
    const id = setTimeout(() => { debouncedQuery = next; }, 150);
    return () => clearTimeout(id);
  });

  $effect(() => {
    const q = debouncedQuery.trim();
    if (q === "") { allHits = []; return; }
    runSearch(q, LIMIT, (hits) => { allHits = hits; });
  });
</script>

<svelte:head>
  <title>Search Online · livtet</title>
</svelte:head>

<wa-page>
  <header slot="header">
    <SearchBar
      bind:value={rawQuery}
      placeholder="Search Hardcover, Google Books, OpenLibrary…"
    />
  </header>

  <main>
    <wa-scroller orientation="vertical" class="result-scroller">
      {#if debouncedQuery.trim() === ""}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          Type a query to search online catalogs.
        </wa-callout>
      {:else if allHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          No results across Google Books, Hardcover, or OpenLibrary.
        </wa-callout>
      {:else}
        <CoverGrid hits={allHits} />
      {/if}
    </wa-scroller>
  </main>
</wa-page>

<style>
  .result-scroller {
    max-height: calc(100vh - 14rem);
  }

  main, header {
    padding: 0;
    margin: 0;
  }
</style>