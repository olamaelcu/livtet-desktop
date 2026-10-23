<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
import type { Component } from 'svelte'

// Renders a route page for SSR specs: no router, no browser, no layout.
// Queries stay pending (nothing awaits them server-side), so specs assert
// the static shell — headings, tabs, landmarks, head content.
let { page: Page }: { page: Component } = $props()

const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider client={client}>
  <Page />
</QueryClientProvider>
