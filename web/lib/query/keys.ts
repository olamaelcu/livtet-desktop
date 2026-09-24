/**
 * Single source of truth for TanStack Query cache keys. Grouping keys by
 * domain (`sync.all`, `search.all`) lets invalidation target a whole domain
 * without enumerating every entry.
 */

export const syncKeys = {
  all: ['sync'] as const,
  health: () => [...syncKeys.all, 'health'] as const,
  status: () => [...syncKeys.all, 'status'] as const,
  pending: () => [...syncKeys.all, 'pending'] as const,
  devices: () => [...syncKeys.all, 'devices'] as const,
  conflicts: () => [...syncKeys.all, 'conflicts'] as const,
  requests: (limit: number) => [...syncKeys.all, 'requests', { limit }] as const,
}

export const searchKeys = {
  all: ['search'] as const,
  editions: (query: string) => [...searchKeys.all, 'editions', { query }] as const,
  typeahead: (query: string) => [...searchKeys.all, 'typeahead', { query }] as const,
}

export const catalogKeys = {
  all: ['catalog'] as const,
  editionDetail: (editionId: string) =>
    [...catalogKeys.all, 'edition-detail', { editionId }] as const,
}
