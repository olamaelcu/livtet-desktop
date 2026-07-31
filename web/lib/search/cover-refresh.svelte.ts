const _refreshed = new Set<string>()

export const refreshState = $state({ version: 0 })

export function triggerCoverRefresh(editionId: string): void {
  _refreshed.add(editionId)
  refreshState.version++
}

export function consumeCoverRefresh(editionId: string): boolean {
  return _refreshed.delete(editionId)
}
