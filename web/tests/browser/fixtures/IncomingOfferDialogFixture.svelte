<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import IncomingOfferDialog from '$lib/components/IncomingOfferDialog.svelte'

// The dialog reads its query client from context. A fresh client per mount
// keeps one test's offers out of the next, and retries are off so a refused
// command surfaces once instead of being retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0, refetchInterval: false },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <IncomingOfferDialog />
</QueryClientProvider>
