<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AppFrame from '$lib/layout/AppFrame.svelte'
import CatalogDetailPage from '$routes/catalog/[catalogId]/+page.svelte'

// The route runs two queries -- the feed and, once a search is submitted, the
// results -- and reads its catalog id from the page params stub.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AppFrame>
    <CatalogDetailPage />
  </AppFrame>
</QueryClientProvider>
