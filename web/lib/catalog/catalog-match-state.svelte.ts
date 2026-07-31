// Module-scoped reactive state for the "already in catalog" dialog.
// The remote-catalog-search component sets this when results contain
// hits that match existing catalog entries. The <AlreadyInCatalogDialog>
// mounts in the layout and reads from it.
import type { SearchHit } from '$lib/search/types'

export const catalogMatchState = $state({
  open: false,
  matches: [] as SearchHit[],
})

export function showCatalogMatches(matches: SearchHit[]): void {
  catalogMatchState.matches = matches
  catalogMatchState.open = true
}

export function closeCatalogMatches(): void {
  catalogMatchState.open = false
}
