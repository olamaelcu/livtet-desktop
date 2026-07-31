<script lang="ts">
  import type { SearchHit } from "../types";
  import CoverGrid from "./cover-grid.svelte";
  import SearchBar from "./search-bar.svelte";

  interface Props {
    title: string;
    placeholder?: string;
    runSearch: (
      query: string,
      limit: number,
      onResults: (hits: SearchHit[]) => void,
      onError?: (error: string) => void,
    ) => Promise<void>;
    limit?: number;
    prompt: string;
    noResults: string;
    filters?: import("svelte").Snippet<
      [{ hits: readonly SearchHit[]; query: string }]
    >;
    result?: import("svelte").Snippet<
      [{ hits: readonly SearchHit[]; query: string }]
    >;
    footer?: import("svelte").Snippet<[{ count: number }]>;
  }

  let {
    title,
    placeholder = "Search the library…",
    runSearch,
    limit = 40,
    prompt,
    noResults,
    filters,
    result,
    footer,
  }: Props = $props();

  let rawQuery = $state("");
  let query = $state("");
  let allHits = $state<SearchHit[]>([]);
  let loading = $state(false);
  let searchError = $state<string | null>(null);

  // Debounce: typing updates rawQuery immediately; query only catches up
  // after 750ms. Returning clearTimeout ensures in-flight timers are
  // cancelled when rawQuery mutates again (effect re-runs).
  $effect(() => {
    const next = rawQuery;
    const id = setTimeout(() => {
      query = next;
    }, 750);
    return () => clearTimeout(id);
  });

  // Search: runs whenever the debounced query changes. Empty query resets
  // state. Non-empty invokes runSearch with a cancelled flag so in-flight
  // work is dropped on cleanup.
  $effect(() => {
    const q = query.trim();
    if (q === "") {
      allHits = [];
      searchError = null;
      loading = false;
      return;
    }
    let cancelled = false;
    loading = true;
    searchError = null;
    runSearch(
      q,
      limit,
      (hits) => {
        if (cancelled) return;
        allHits = hits;
        loading = false;
      },
      (err) => {
        if (cancelled) return;
        searchError = err;
        loading = false;
      },
    );
    return () => {
      cancelled = true;
    };
  });
</script>

<svelte:head>
  <title>{title}</title>
</svelte:head>

<wa-page>
  <header slot="header">
    <SearchBar bind:value={rawQuery} {placeholder} />
  </header>

  {#if filters}
    <div slot="main-header">
      {@render filters({ hits: allHits, query })}
    </div>
  {/if}

  <main>
    <wa-scroller orientation="vertical" class="result-scroller">
      {#if searchError}
        <wa-callout variant="warning">
          <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
          Search failed: {searchError}
        </wa-callout>
      {:else if query.trim() === ""}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          {prompt}
        </wa-callout>
      {:else if loading && allHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="hourglass"></wa-icon>
          Searching…
        </wa-callout>
      {:else if result}
        {@render result({ hits: allHits, query })}
      {:else if allHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          {noResults}
        </wa-callout>
      {:else}
        <CoverGrid hits={allHits} />
      {/if}
    </wa-scroller>
  </main>

  {#if footer}
    <footer slot="main-footer">
      {@render footer({ count: allHits.length })}
    </footer>
  {/if}
</wa-page>

<style>
  .result-scroller {
    max-height: calc(100vh - 20rem);
  }

  main {
    padding: var(--wa-space-s);
    margin: 0;
    flex: 1 1;
    min-height: max-content;
  }

  div[slot="main-header"] {
    padding: var(--wa-space-m);
    margin: 0;
    flex-basis: 100%;
  }
</style>
