<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { searchKeys } from '$lib/query/keys'
import { refreshLibraryOnPull } from '$lib/sync/library'

/**
 * The library page's fetch, reduced to what the test reads: one cached page of
 * books, and the pull subscription the layout installs.
 */
const queryClient = useQueryClient()

let fetches = 0

const editions = createQuery(() => ({
  queryKey: searchKeys.editions('', {}),
  queryFn: async () => {
    fetches += 1
    return [`book ${fetches}`]
  },
  staleTime: Number.POSITIVE_INFINITY,
}))

onMount(() => {
  void refreshLibraryOnPull(queryClient)
})
</script>

<ul class="library">
  {#each editions.data ?? [] as book (book)}
    <li>{book}</li>
  {/each}
</ul>
