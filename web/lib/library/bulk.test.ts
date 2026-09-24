import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { deleteEditions, matchingEditionIds } from './bulk'

describe('bulk wrappers', () => {
  beforeEach(() => invoke.mockReset())

  it('deleteEditions short-circuits an empty list', async () => {
    await expect(deleteEditions([])).resolves.toEqual({
      deleted: 0,
      files_removed: 0,
      covers_removed: 0,
      skipped: [],
    })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('deleteEditions forwards ids', async () => {
    invoke.mockResolvedValueOnce({ deleted: 2, files_removed: 1, covers_removed: 1, skipped: [] })
    await deleteEditions(['a', 'b'])
    expect(invoke).toHaveBeenCalledWith('delete_editions', { editionIds: ['a', 'b'] })
  })

  it('matchingEditionIds forwards query and filters', async () => {
    invoke.mockResolvedValueOnce(['e1'])
    await matchingEditionIds('q', { tag_ids: ['t'] })
    expect(invoke).toHaveBeenCalledWith('matching_edition_ids', {
      query: 'q',
      filters: { tag_ids: ['t'] },
    })
  })
})
