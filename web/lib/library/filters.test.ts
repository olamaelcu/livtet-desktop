import { describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke, convertFileSrc: (p: string) => `asset://${p}` }))

import { searchKeys } from '../query/keys'
import { loadEditions } from '../search'

describe('searchKeys.editions', () => {
  it('is stable across key order and varies with filters', () => {
    const a = searchKeys.editions('q', { format_ids: ['1'], tag_ids: ['2'] } as never)
    const b = searchKeys.editions('q', { tag_ids: ['2'], format_ids: ['1'] } as never)
    expect(JSON.stringify(a)).toBe(JSON.stringify(b))
    expect(JSON.stringify(a)).not.toBe(JSON.stringify(searchKeys.editions('q', {})))
  })
})

describe('loadEditions', () => {
  it('forwards filters to the command', async () => {
    invoke.mockResolvedValueOnce({ hits: [], total: 0 })
    await loadEditions('q', { tag_ids: ['t'] } as never, 0, 20)
    expect(invoke).toHaveBeenCalledWith('search_editions', {
      query: 'q',
      filters: { tag_ids: ['t'] },
      offset: 0,
      limit: 20,
    })
  })
})
