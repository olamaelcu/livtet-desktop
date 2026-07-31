import { describe, expect, it } from 'vitest'
import { emptyFilters } from './types'

describe('emptyFilters', () => {
  it('returns empty Set for formats', () => {
    expect(emptyFilters().formats.size).toBe(0)
  })

  it('returns empty Set for languages', () => {
    expect(emptyFilters().languages.size).toBe(0)
  })

  it('returns a new object each call', () => {
    const a = emptyFilters()
    const b = emptyFilters()
    expect(a).not.toBe(b)
  })
})
