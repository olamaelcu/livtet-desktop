<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import RouteShell from '$lib/layout/RouteShell.svelte'

// RouteShell hosts the nav rail, which reads its badges through createQuery.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <RouteShell>
    <div data-testid="body">body</div>
  </RouteShell>
</QueryClientProvider>
