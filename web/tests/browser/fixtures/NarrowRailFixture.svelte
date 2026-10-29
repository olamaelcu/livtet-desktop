<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import NavRail from '$lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '$lib/layout/navItems'

let { width }: { width: string } = $props()

// NavRail reads its nav badges through createQuery, so it needs a provider.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <div data-testid="container" style="container-type: inline-size; width: {width}">
    <NavRail items={NAV_ITEMS} currentPath="/library" expanded />
  </div>
</QueryClientProvider>
