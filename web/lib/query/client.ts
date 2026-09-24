import { QueryClient } from '@tanstack/svelte-query'

const MINUTE = 60_000

/**
 * Desktop-tuned defaults for data served over local Tauri IPC: reads are cheap
 * and there is no network to wait on, so staleness windows are short, and the
 * window is effectively always focused.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5_000,
      gcTime: 5 * MINUTE,
      refetchOnWindowFocus: false,
      retry: 2,
      retryDelay: (attempt) => Math.min(500 * 2 ** attempt, 5_000),
    },
    mutations: {
      retry: 0,
    },
  },
})
