import { beforeEach, describe, expect, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  getReadingList,
  listReadingLists,
  readingListErrorMessage,
  reorderReadingList,
  updateReadingList,
} from './reading-lists'

beforeEach(() => {
  invoke.mockReset()
})

describe('readingListErrorMessage', () => {
  test('returns a plain string error as-is', () => {
    expect(readingListErrorMessage('boom')).toBe('boom')
  })

  test('reads the engine message off a ReadingListError', () => {
    expect(
      readingListErrorMessage({
        code: 'invalid_input',
        message: 'invalid input: duplicate edition 01ABC in reorder of reading list 01DEF',
      }),
    ).toBe('invalid input: duplicate edition 01ABC in reorder of reading list 01DEF')
  })

  test('falls back for unrecognized shapes', () => {
    expect(readingListErrorMessage(null)).toBe('Reading list request failed')
    expect(readingListErrorMessage({})).toBe('Reading list request failed')
    expect(readingListErrorMessage({ message: '' })).toBe('Reading list request failed')
    expect(readingListErrorMessage(42)).toBe('Reading list request failed')
  })
})

test('listReadingLists calls its command', async () => {
  invoke.mockResolvedValue([])
  await expect(listReadingLists()).resolves.toEqual([])
  expect(invoke).toHaveBeenCalledWith('list_reading_lists')
})

test('getReadingList forwards the list id', async () => {
  const list = {
    id: '01LIST',
    name: 'Autumn',
    description: null,
    edition_ids: [],
    created_at: '2026-11-04T00:00:00Z',
    updated_at: null,
  }
  invoke.mockResolvedValue(list)
  await expect(getReadingList('01LIST')).resolves.toEqual(list)
  expect(invoke).toHaveBeenCalledWith('get_reading_list', { listId: '01LIST' })
})

test('updateReadingList forwards the patch', async () => {
  invoke.mockResolvedValue({
    id: '01LIST',
    name: 'Winter',
    description: '',
    edition_ids: [],
    created_at: '2026-11-04T00:00:00Z',
    updated_at: '2026-11-04T00:01:00Z',
  })
  await updateReadingList('01LIST', { name: 'Winter', description: '' })
  expect(invoke).toHaveBeenCalledWith('update_reading_list', {
    listId: '01LIST',
    patch: { name: 'Winter', description: '' },
  })
})

test('reorderReadingList submits the whole ordered edition id list', async () => {
  invoke.mockResolvedValue({
    id: '01LIST',
    name: 'Autumn',
    description: null,
    edition_ids: ['01B', '01A'],
    created_at: '2026-11-04T00:00:00Z',
    updated_at: '2026-11-04T00:01:00Z',
  })
  await reorderReadingList('01LIST', ['01B', '01A'])
  expect(invoke).toHaveBeenCalledWith('reorder_reading_list', {
    listId: '01LIST',
    editionIds: ['01B', '01A'],
  })
})
