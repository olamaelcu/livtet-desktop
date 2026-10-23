<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import FilterPanel from '$lib/library/FilterPanel.svelte'
import type { EditionFilters } from '$lib/search'

interface Props {
  filters?: EditionFilters
  onchange?: (filters: EditionFilters) => void
}

let {
  filters = {},
  onchange = () => {},
  onclose = () => {},
}: Props & { onclose?: () => void } = $props()

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <FilterPanel {filters} {onchange} {onclose} />
</QueryClientProvider>
