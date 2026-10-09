<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import LibraryPage from '$routes/library/+page.svelte'

// The route reads its query client from context and drives an infinite query
// plus a filter-options query. A fresh client per mount keeps the cache from
// leaking between tests; retries are off so a refused command surfaces once
// rather than being retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0, refetchInterval: false },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <LibraryPage />
</QueryClientProvider>
