import { describe, expect, it } from 'vitest'
import { hit } from '$lib/test-fixtures'
import { filterHits } from './search'
import { emptyFilters, type FilterState } from './types'

describe('filterHits', () => {
  const hits = [
    hit({
      title: 'The Hobbit',
      authors: ['J.R.R. Tolkien'],
      language: 'en',
      format: 'EPUB',
      snippet_text: 'In a hole in the ground there lived a hobbit',
    }),
    hit({
      title: 'Le Petit Prince',
      authors: ['Antoine de Saint-Exupéry'],
      language: 'fr',
      format: 'PDF',
    }),
    hit({ title: 'El Quijote', authors: ['Miguel de Cervantes'], language: 'es', format: 'EPUB' }),
    hit({ title: 'No Language', authors: ['Test Author'], language: null, format: null }),
  ]

  it('returns all hits when query is empty and filters are empty', () => {
    expect(filterHits('', emptyFilters(), hits)).toHaveLength(4)
  })

  it('filters by title query (case-insensitive)', () => {
    const result = filterHits('hobbit', emptyFilters(), hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('The Hobbit')
  })

  it('filters by author name', () => {
    const result = filterHits('Cervantes', emptyFilters(), hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('El Quijote')
  })

  it('filters by snippet text', () => {
    const result = filterHits('hole in the ground', emptyFilters(), hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('The Hobbit')
  })

  it('matches partial words', () => {
    const result = filterHits('hob', emptyFilters(), hits)
    expect(result).toHaveLength(1)
  })

  it('trims query whitespace', () => {
    const result = filterHits('  hobbit  ', emptyFilters(), hits)
    expect(result).toHaveLength(1)
  })

  it('filters by format', () => {
    const filters: FilterState = { formats: new Set(['PDF']), languages: new Set() }
    const result = filterHits('', filters, hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('Le Petit Prince')
  })

  it('filters by language', () => {
    const filters: FilterState = { formats: new Set(), languages: new Set(['fr']) }
    const result = filterHits('', filters, hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('Le Petit Prince')
  })

  it('combines format and language filters', () => {
    const filters: FilterState = { formats: new Set(['EPUB']), languages: new Set(['en']) }
    const result = filterHits('', filters, hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('The Hobbit')
  })

  it('combines query and filters', () => {
    const filters: FilterState = { formats: new Set(['EPUB']), languages: new Set() }
    const result = filterHits('quijote', filters, hits)
    expect(result).toHaveLength(1)
    expect(result[0].title).toBe('El Quijote')
  })

  it('returns empty when no format matches', () => {
    const filters: FilterState = { formats: new Set(['MOBI']), languages: new Set() }
    expect(filterHits('', filters, hits)).toHaveLength(0)
  })

  it('returns empty when no language matches', () => {
    const filters: FilterState = { formats: new Set(), languages: new Set(['de']) }
    expect(filterHits('', filters, hits)).toHaveLength(0)
  })

  it('handles empty hits array', () => {
    expect(filterHits('anything', emptyFilters(), [])).toHaveLength(0)
  })

  it('handles hits with null format/language when filtering', () => {
    const filters: FilterState = { formats: new Set(['EPUB']), languages: new Set() }
    const result = filterHits('', filters, hits)
    expect(result.every((h) => h.format === 'EPUB')).toBe(true)
  })
})
