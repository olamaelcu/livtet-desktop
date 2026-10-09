<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AppFrame from '$lib/layout/AppFrame.svelte'
import CatalogIndexPage from '../../../routes/catalog/+page.svelte'

// The route loads its catalogs through a query and renders inside the app
// frame, like the other list routes. Retries are off so a refused command
// reaches the error branch once instead of being retried behind the assertion.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AppFrame>
    <CatalogIndexPage />
  </AppFrame>
</QueryClientProvider>
