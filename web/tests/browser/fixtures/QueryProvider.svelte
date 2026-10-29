<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'

// The `wrapper` option of vitest-browser-svelte's `render`, for tests that mount
// a component directly instead of through a fixture. Retries are off so a
// refused command reaches the error branch exactly once.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})

let { children } = $props()
</script>

<QueryClientProvider {client}>
  {@render children()}
</QueryClientProvider>
