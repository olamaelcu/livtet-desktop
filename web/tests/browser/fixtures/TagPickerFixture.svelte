<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import TagPicker from '$lib/library/TagPicker.svelte'

interface Props {
  onadd?: (tag: string) => void
  onremove?: (tagId: string) => void
  onclose?: () => void
}

// TagPicker loads the tag list through a query, so it needs a provider. The
// callbacks are forwarded so the test can assert on what the picker reports.
let { onadd = () => {}, onremove = () => {}, onclose = () => {} }: Props = $props()

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <TagPicker {onadd} {onremove} {onclose} />
</QueryClientProvider>
