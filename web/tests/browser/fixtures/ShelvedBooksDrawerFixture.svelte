<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import ShelvedBooksDrawer from '$lib/library/ShelvedBooksDrawer.svelte'

interface Props {
  open?: boolean
  onclose?: () => void
}

let { open = true, onclose = () => {} }: Props = $props()

// The drawer reads the account's shelving through one query; the spec mocks
// `invoke`, so no client calls leave the page.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <ShelvedBooksDrawer {open} {onclose} />
</QueryClientProvider>
