<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AddPluginPanel from '$lib/components/AddPluginPanel.svelte'

// The panel drives three mutations and invalidates the installed-plugin query
// on success, so it needs a provider. Retries are off so a refused command
// reaches the error branch once.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AddPluginPanel />
</QueryClientProvider>
