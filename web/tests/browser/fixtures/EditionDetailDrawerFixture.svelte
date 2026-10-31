<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import EditionDetailDrawer from '$lib/library/EditionDetailDrawer.svelte'

interface Props {
  editionId?: string
  open?: boolean
  onclose?: () => void
}

let { editionId = '01hzzzzzzzzzzzzzzzzzzzzzzz', open = true, onclose = () => {} }: Props = $props()

// The drawer reads one edition-detail query; the spec mocks `invoke`.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <EditionDetailDrawer {editionId} {open} {onclose} />
</QueryClientProvider>
