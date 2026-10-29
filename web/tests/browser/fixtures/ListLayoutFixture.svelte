<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AppFrame from '$lib/layout/AppFrame.svelte'
import ListLayout from '$lib/layout/ListLayout.svelte'

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
    <ListLayout title="Things">
      {#snippet action()}
        <wa-button>Act</wa-button>
      {/snippet}
      <div style="height: 3000px">tall</div>
    </ListLayout>
  </AppFrame>
</QueryClientProvider>
