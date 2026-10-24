<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AccountSettings from '$lib/components/AccountSettings.svelte'

// Status and profile queries plus three mutations read the client from
// context. Retries off so a refused command surfaces once.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AccountSettings />
</QueryClientProvider>
