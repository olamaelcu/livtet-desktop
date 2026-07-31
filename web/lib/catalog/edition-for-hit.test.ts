import { describe, expect, it } from 'vitest'
import type { DigitalInventoryRow } from '$lib/bindings'
import { hit } from '$lib/test-fixtures'
import { editionForHit } from './edition-for-hit'

describe('editionForHit', () => {
  it('returns files.edition_id when files are provided', () => {
    const h = hit({ edition_id: null })
    const files: DigitalInventoryRow = {
      id: 'inv-1',
      edition_id: 'edition-42',
      file_path: null,
      cover_path: null,
      blurhash: null,
      dominant_color: null,
      file_hash: null,
      file_size_bytes: null,
      file_format: null,
      notes: null,
      added_at: '2024-01-01T00:00:00Z',
      updated_at: null,
    }
    expect(editionForHit(h, files)).toEqual({ editionId: 'edition-42' })
  })

  it('falls back to hit.edition_id when files is null', () => {
    const h = hit({ edition_id: 'edition-7' })
    expect(editionForHit(h, null)).toEqual({ editionId: 'edition-7' })
  })

  it('falls back to hit.edition_id when files is undefined', () => {
    const h = hit({ edition_id: 'edition-7' })
    expect(editionForHit(h, undefined)).toEqual({ editionId: 'edition-7' })
  })

  it('returns null editionId when both are null/undefined', () => {
    const h = hit({ edition_id: null })
    expect(editionForHit(h, null)).toEqual({ editionId: null })
    expect(editionForHit(h, undefined)).toEqual({ editionId: null })
  })
})
