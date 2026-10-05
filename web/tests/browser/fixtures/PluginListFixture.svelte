<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import PluginList from '../../../lib/components/PluginList.svelte'

// PluginList reads its query client from context, so the component under test
// needs a provider. A fresh client per mount keeps tests from sharing a cache,
// and retries are off so a refused command surfaces once instead of being
// retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <PluginList />
</QueryClientProvider>
