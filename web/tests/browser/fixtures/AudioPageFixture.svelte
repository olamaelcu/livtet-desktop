<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import AudioPage from '$routes/reader/audio/[editionId]/+page.svelte'

// The page reads the edition id from the page params stub and loads the
// publication, detail, covers, and saved progress; the spec mocks `invoke`.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AudioPage />
</QueryClientProvider>
