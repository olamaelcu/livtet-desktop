import { describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke, convertFileSrc: (p: string) => `asset://${p}` }))

import { searchKeys } from '../query/keys'
import { loadEditions } from '../search'

describe('searchKeys.editions', () => {
  it('is stable across key order and varies with filters', () => {
    const a = searchKeys.editions('q', { format_ids: ['1'], tag_ids: ['2'] })
    const b = searchKeys.editions('q', { tag_ids: ['2'], format_ids: ['1'] })
    expect(JSON.stringify(a)).toBe(JSON.stringify(b))
    expect(JSON.stringify(a)).not.toBe(JSON.stringify(searchKeys.editions('q', {})))
  })

  it('canonicalizes array order and drops empty axes', () => {
    const a = searchKeys.editions('q', { tag_ids: ['b', 'a'] })
    const b = searchKeys.editions('q', { tag_ids: ['a', 'b'] })
    expect(JSON.stringify(a)).toBe(JSON.stringify(b))
    expect(JSON.stringify(searchKeys.editions('q', { tag_ids: [] }))).toBe(
      JSON.stringify(searchKeys.editions('q', {})),
    )
  })
})

describe('loadEditions', () => {
  it('forwards filters to the command', async () => {
    invoke.mockResolvedValueOnce({ hits: [], total: 0 })
    await loadEditions('q', { tag_ids: ['t'] }, 0, 20)
    expect(invoke).toHaveBeenCalledWith('search_editions', {
      query: 'q',
      filters: { tag_ids: ['t'] },
      offset: 0,
      limit: 20,
    })
  })
})
