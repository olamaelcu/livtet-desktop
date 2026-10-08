<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import SyncPanel from '../../../lib/components/SyncPanel.svelte'

// SyncPanel reads its query client from context. A fresh client per mount keeps
// its six queries from sharing a cache between tests, and retries are off so a
// refused command surfaces once instead of being retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0, refetchInterval: false },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <SyncPanel />
</QueryClientProvider>
