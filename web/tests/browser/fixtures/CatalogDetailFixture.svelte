<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AppFrame from '$lib/layout/AppFrame.svelte'
import CatalogDetailLayout from '$routes/catalog/[catalogId]/CatalogDetailLayout.svelte'

// AppFrame hosts the nav rail, which reads its badges through createQuery.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AppFrame>
    <CatalogDetailLayout>
      {#snippet toolbar()}
        <wa-button>Back to root</wa-button>
      {/snippet}
      <div style="height: 3000px">publications</div>
    </CatalogDetailLayout>
  </AppFrame>
</QueryClientProvider>
