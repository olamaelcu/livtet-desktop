const _refreshed = new Set<string>()

export function triggerCoverRefresh(editionId: string): void {
  _refreshed.add(editionId)
  coverRefreshVersion++
}

export function consumeCoverRefresh(editionId: string): boolean {
  return _refreshed.delete(editionId)
}

let coverRefreshVersion = $state(0)

export { coverRefreshVersion }
