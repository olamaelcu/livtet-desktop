<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AudioPage from '$routes/reader/audio/[editionId]/+page.svelte'

// The route reads its query client from context and drives the publication,
// cover, progress and edition-detail queries. A fresh client per mount keeps
// the cache from leaking between tests; retries are off so a refused command
// surfaces once instead of being retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0, refetchInterval: false },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AudioPage />
</QueryClientProvider>
