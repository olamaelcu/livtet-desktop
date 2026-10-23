import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  addEditionTags,
  deleteEditions,
  exportEditionsCsv,
  matchingEditionIds,
  removeEditionTags,
} from './bulk'

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

  it('exportEditionsCsv forwards ids and path', async () => {
    invoke.mockResolvedValueOnce({ path: '/tmp/out.csv', rows: 2 })
    await exportEditionsCsv(['a'], '/tmp/out.csv')
    expect(invoke).toHaveBeenCalledWith('export_editions_csv', {
      editionIds: ['a'],
      path: '/tmp/out.csv',
    })
  })

  it('addEditionTags and removeEditionTags forward their payloads', async () => {
    invoke.mockResolvedValue({ updated: 1, added: [], removed: [] })
    await addEditionTags(['a'], 'fiction')
    expect(invoke).toHaveBeenCalledWith('add_edition_tags', { editionIds: ['a'], tag: 'fiction' })
    await removeEditionTags(['a'], 'tag-1')
    expect(invoke).toHaveBeenCalledWith('remove_edition_tags', {
      editionIds: ['a'],
      tagId: 'tag-1',
    })
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
