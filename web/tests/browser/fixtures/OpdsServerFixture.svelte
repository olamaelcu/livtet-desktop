<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import OpdsServerSettings from '$lib/components/OpdsServerSettings.svelte'

// One query plus one mutation, both reading the client from context. Retries
// off so a refused command reaches its toast once.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <OpdsServerSettings />
</QueryClientProvider>
