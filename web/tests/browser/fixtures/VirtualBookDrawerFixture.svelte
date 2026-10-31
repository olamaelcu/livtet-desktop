<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import VirtualBookDrawer from '$lib/library/VirtualBookDrawer.svelte'

interface Props {
  open?: boolean
  /** Records every callback the drawer fires so a spec can assert on it. */
  events?: string[]
}

let { open = true, events = [] }: Props = $props()

// The drawer invalidates the search cache after a create, so it needs a client.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <VirtualBookDrawer {open} onclose={() => events.push('close')} />
</QueryClientProvider>
