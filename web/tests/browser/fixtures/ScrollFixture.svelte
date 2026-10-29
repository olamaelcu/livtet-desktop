<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import Pane from '$lib/layout/Pane.svelte'
import ScrollRegion from '$lib/layout/ScrollRegion.svelte'

let { onscroll }: { onscroll?: () => void } = $props()

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <div id="host" style="height: 200px; display: grid" {onscroll}>
    <Pane>
      <ScrollRegion>
        <div style="height: 1000px">tall</div>
      </ScrollRegion>
    </Pane>
  </div>
</QueryClientProvider>
