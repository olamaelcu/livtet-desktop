<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import CatalogSettings from '$lib/components/CatalogSettings.svelte'

// Two queries (catalogs, presets) plus four mutations, all reading the client
// from context. Retries off so a refused command reaches its toast once.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <CatalogSettings />
</QueryClientProvider>
