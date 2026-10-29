<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
// Renders the real PDF reader route inside the app frame, so the layout
// contract is asserted against the page itself rather than a hand-copy of it.
// pdf.js and the Tauri IPC are mocked by the test.
import AppFrame from '$lib/layout/AppFrame.svelte'
import PdfReaderPage from '$routes/reader/pdf/[editionId]/+page.svelte'

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
    <PdfReaderPage />
  </AppFrame>
</QueryClientProvider>
