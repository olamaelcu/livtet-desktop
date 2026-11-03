import type { QueryClient } from '@tanstack/svelte-query'
import type { UnlistenFn } from '@tauri-apps/api/event'

import { catalogKeys, searchKeys } from '../query/keys'
import { listenToSyncEvents } from '../sync'

/**
 * Refetch the library when a pull brought it new books.
 *
 * The daemon pulls on its own clock, so nothing in the web layer knows a book
 * arrived: `sync://pulled` is the daemon reporting that it applied rows here.
 * From that moment every cached page, filter count, and cover is stale, so the
 * whole catalog domain is dropped rather than the one key this module could
 * guess — the library view builds its own query shapes from the active filters.
 */
export function refreshLibraryOnPull(queryClient: QueryClient): Promise<UnlistenFn> {
  return listenToSyncEvents({
    'sync://pulled': () => {
      void queryClient.invalidateQueries({ queryKey: searchKeys.all })
      void queryClient.invalidateQueries({ queryKey: catalogKeys.all })
    },
  })
}
