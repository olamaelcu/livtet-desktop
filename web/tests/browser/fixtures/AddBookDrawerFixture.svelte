<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AddBookDrawer from '../../../lib/library/AddBookDrawer.svelte'

// AddBookDrawer reads its query client from context, so it needs a provider.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AddBookDrawer open={true} onclose={() => {}} />
</QueryClientProvider>
