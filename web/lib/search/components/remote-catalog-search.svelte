<script lang="ts">
  import { onMount } from 'svelte'
  import { runSearch, subscribeProviderFailures } from '$lib/remote/chain'
  import SearchView from '$lib/search/components/search-view.svelte'

  interface Props {
    f?: string
  }

  let { f = $bindable("") }: Props = $props()

  const LIMIT = 50

  onMount(() => {
    subscribeProviderFailures()
  })
</script>

<SearchView
  title="Search Online · livtet"
  placeholder="Search Hardcover, Google Books, OpenLibrary…"
  limit={LIMIT}
  {runSearch}
  prompt="Type a query to search online catalogs."
  noResults="No results across Google Books, Hardcover, or OpenLibrary."
/>
