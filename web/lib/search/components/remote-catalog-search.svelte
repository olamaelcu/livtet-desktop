<script lang="ts">
import { onMount } from 'svelte'
import { showCatalogMatches } from '$lib/catalog/catalog-match-state.svelte'
import { runSearch, subscribeProviderFailures } from '$lib/remote/chain'
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
  onError?: (error: string) => void,
): Promise<void> {
  await runSearch(
    query,
    limit,
    (hits) => {
      onHits(hits)
      onResults(hits)
    },
    onError,
  )
}
</script>

<SearchView
  title="Search Online · livtet"
  placeholder="Search Hardcover, Google Books, OpenLibrary…"
  limit={LIMIT}
  runSearch={wrappedRunSearch}
  prompt="Type a query to search online catalogs."
  noResults="No results across Google Books, Hardcover, or OpenLibrary."
/>
