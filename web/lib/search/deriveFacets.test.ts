import { describe, expect, it } from 'vitest'
import { hit } from '$lib/test-fixtures'
import { deriveFacets } from './deriveFacets'

describe('deriveFacets', () => {
  it('returns empty arrays for empty hit list', () => {
    const { formats, languages } = deriveFacets([])
    expect(formats).toEqual([])
    expect(languages).toEqual([])
  })

  it('extracts unique formats sorted alphabetically', () => {
    const hits = [
      hit({ format: 'PDF' }),
      hit({ format: 'EPUB' }),
      hit({ format: 'PDF' }),
      hit({ format: 'CBZ' }),
    ]
    expect(deriveFacets(hits).formats).toEqual(['CBZ', 'EPUB', 'PDF'])
  })

  it('extracts unique languages sorted alphabetically', () => {
    const hits = [
      hit({ language: 'en' }),
      hit({ language: 'fr' }),
      hit({ language: 'en' }),
      hit({ language: 'es' }),
    ]
    expect(deriveFacets(hits).languages).toEqual(['en', 'es', 'fr'])
  })

  it('filters out null formats and languages', () => {
    const hits = [
      hit({ format: 'EPUB', language: 'en' }),
      hit({ format: null, language: null }),
      hit({ format: 'PDF', language: 'fr' }),
    ]
    const { formats, languages } = deriveFacets(hits)
    expect(formats).toEqual(['EPUB', 'PDF'])
    expect(languages).toEqual(['en', 'fr'])
  })

  it('handles all-null values', () => {
    const hits = [hit({ format: null, language: null }), hit({ format: null, language: null })]
    const { formats, languages } = deriveFacets(hits)
    expect(formats).toEqual([])
    expect(languages).toEqual([])
  })

  it('localeCompare sorts with mixed case (default locale)', () => {
    const hits = [hit({ format: 'epub' }), hit({ format: 'EPUB' }), hit({ format: 'Cbz' })]
    const formats = deriveFacets(hits).formats
    // localeCompare is locale-dependent; verify sorted order
    expect(formats).toEqual(formats.slice().sort((a, b) => a.localeCompare(b)))
  })
})
