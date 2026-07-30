<script lang="ts">
  import SearchView from "$lib/search/components/search-view.svelte";
  import { runSearch } from "$lib/remote/chain";

  const LIMIT = 50;
</script>

<SearchView
  title="Search Online · livtet"
  placeholder="Search Hardcover, Google Books, OpenLibrary…"
  limit={LIMIT}
  runSearch={runSearch}
  prompt="Type a query to search online catalogs."
  noResults="No results across Google Books, Hardcover, or OpenLibrary."
/>
