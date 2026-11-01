<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import PullRefreshPage from './PullRefreshPage.svelte'

// A fresh client per mount keeps one test's cached page out of the next, and
// retries are off so a refused fetch surfaces once instead of being retried
// behind the assertion.
const client = new QueryClient({
  defaultOptions: { queries: { retry: false, gcTime: 0 } },
})
</script>

<QueryClientProvider {client}>
  <PullRefreshPage />
</QueryClientProvider>
