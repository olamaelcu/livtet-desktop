<script lang="ts">
import { onMount } from 'svelte'
import { showCatalogMatches } from '$lib/catalog/catalog-match-state.svelte'
import { openPeek } from '$lib/catalog/peek-state.svelte'
import { runSearch, subscribeProviderFailures } from '$lib/remote/chain'
import CoverGrid from '$lib/search/components/cover-grid.svelte'
import ImportAction from '$lib/search/components/import-action.svelte'
import SearchView from '$lib/search/components/search-view.svelte'
import type { SearchHit } from '$lib/search/types'

interface Props {
  f?: string
}

let { f = $bindable('') }: Props = $props()

const LIMIT = 50

onMount(() => {
  subscribeProviderFailures()
})

function onResults(hits: SearchHit[]): void {
  const matches = hits.filter((h) => h.in_catalog)
  if (matches.length > 0) {
    showCatalogMatches(matches)
  }
}

async function wrappedRunSearch(
  query: string,
  limit: number,
  onHits: (hits: SearchHit[]) => void,
  _onError?: (error: string) => void,
): Promise<void> {
  await runSearch(query, limit, (hits) => {
    onHits(hits)
    onResults(hits)
  })
}

function onBadgeClick(hit: SearchHit, e: Event): void {
  e.stopPropagation()
  if (hit.in_catalog_edition_id) openPeek(hit.in_catalog_edition_id)
}
</script>

{#snippet badge(hit: SearchHit)}
  {#if hit.in_catalog}
    <wa-badge
      variant="success"
      appearance="filled"
      class="in-catalog-badge"
      onclick={(e: Event) => onBadgeClick(hit, e)}
    >
      In catalog
    </wa-badge>
  {/if}
{/snippet}

{#snippet actions(hit: SearchHit)}
  <ImportAction {hit} />
{/snippet}

<SearchView
  title="Search Online · livtet"
  placeholder="Search Hardcover, Google Books, OpenLibrary…"
  limit={LIMIT}
  runSearch={wrappedRunSearch}
  prompt="Type a query to search online catalogs."
  noResults="No results across Google Books, Hardcover, or OpenLibrary."
>
  {#snippet result(input: { hits: readonly SearchHit[]; query: string })}
    <CoverGrid hits={input.hits} badge={badge} actions={actions} />
  {/snippet}
</SearchView>

<style>
  .in-catalog-badge {
    --wa-badge-font-size: 0.6875rem;
    --wa-badge-padding: 0 0.375rem;
    cursor: pointer;
  }
</style>
