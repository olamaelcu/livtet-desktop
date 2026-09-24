import { describe, expect, it } from 'vitest'
import { searchKeys, syncKeys } from './keys'

describe('syncKeys', () => {
  it('groups every sync key under the sync root', () => {
    expect(syncKeys.health()).toEqual(['sync', 'health'])
    expect(syncKeys.status()).toEqual(['sync', 'status'])
    expect(syncKeys.pending()).toEqual(['sync', 'pending'])
    expect(syncKeys.devices()).toEqual(['sync', 'devices'])
    expect(syncKeys.conflicts()).toEqual(['sync', 'conflicts'])
    expect(syncKeys.requests(50)).toEqual(['sync', 'requests', { limit: 50 }])
  })

  it('nests the domain root so it can invalidate every entry at once', () => {
    expect(syncKeys.all).toEqual(['sync'])
    expect(syncKeys.status().slice(0, 1)).toEqual(syncKeys.all)
  })

  it('distinguishes request limits', () => {
    expect(syncKeys.requests(10)).not.toEqual(syncKeys.requests(50))
  })
})

describe('searchKeys', () => {
  it('partitions editions and typeahead by query', () => {
    expect(searchKeys.editions('dune')).toEqual(['search', 'editions', { query: 'dune' }])
    expect(searchKeys.typeahead('dune')).toEqual(['search', 'typeahead', { query: 'dune' }])
  })

  it('produces distinct keys for distinct queries', () => {
    expect(searchKeys.editions('dune')).not.toEqual(searchKeys.editions('neuromancer'))
  })
})
