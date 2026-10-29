<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import type { HighlightColor } from '$lib/reader/highlights'

interface Props {
  quote?: string
  showColors?: boolean
  initialText?: string
  initialColor?: HighlightColor
  saved: Array<{ text: string; color: HighlightColor | undefined }>
}

let { quote, showColors = false, initialText, initialColor, saved }: Props = $props()

// The composer asks for the ATProto status through createQuery to decide whether
// it can publish, so it needs a provider.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AnnotationComposer
    placeholder="Note text"
    {quote}
    {showColors}
    {initialText}
    {initialColor}
    onsave={(text, color) => {
      saved.push({ text, color })
    }}
    oncancel={() => {
      saved.push({ text: '<cancel>', color: undefined })
    }}
  />
</QueryClientProvider>
