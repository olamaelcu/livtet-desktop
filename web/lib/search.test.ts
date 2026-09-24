import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke, convertFileSrc } = vi.hoisted(() => ({
  invoke: vi.fn(),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${path}`),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke, convertFileSrc }))

import { coverUrlFor, loadEditionDetail, mapHitToEdition } from './search'

const editionHit = {
  work_id: 'w',
  edition_id: 'e',
  title: 'The Book',
  authors: ['Ada Lovelace'],
  pub_date: '2001-09-11',
  snippet_text: 'A snippet',
  kind: 'edition' as const,
  has_file: true,
}

describe('mapHitToEdition', () => {
  it('carries the edition id and kind', () => {
    const edition = mapHitToEdition(editionHit)
    expect(edition.id).toBe('e')
    expect(edition.edition_id).toBe('e')
    expect(edition.kind).toBe('edition')
  })

  it('falls back to the work id and a null edition id for work hits', () => {
    const edition = mapHitToEdition({ ...editionHit, edition_id: null, kind: 'work' })
    expect(edition.id).toBe('w')
    expect(edition.edition_id).toBeNull()
  })
})

describe('loadEditionDetail', () => {
  beforeEach(() => invoke.mockReset())

  it('invokes get_edition_detail with the edition id', async () => {
    invoke.mockResolvedValueOnce(null)
    await loadEditionDetail('e')
    expect(invoke).toHaveBeenCalledWith('get_edition_detail', { editionId: 'e' })
  })
})

describe('coverUrlFor', () => {
  it('converts present paths and ignores absent ones', () => {
    expect(coverUrlFor('/covers/x.jpg')).toBe('asset://localhost//covers/x.jpg')
    expect(coverUrlFor(null)).toBeUndefined()
    expect(coverUrlFor(undefined)).toBeUndefined()
  })
})
