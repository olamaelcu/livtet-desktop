<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import ReadingListDetailPage from '$routes/reading-lists/[id]/+page.svelte'

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0, refetchInterval: false },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <ReadingListDetailPage />
</QueryClientProvider>
