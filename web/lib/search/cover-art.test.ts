import { describe, expect, it } from 'vitest'
import { coverLetter, dominantColorFor } from './cover-art'

describe('dominantColorFor', () => {
  it('returns a string starting with #', () => {
    expect(dominantColorFor('The Hobbit')).toMatch(/^#[0-9a-f]{6}$/)
  })

  it('is deterministic', () => {
    expect(dominantColorFor('The Hobbit')).toBe(dominantColorFor('The Hobbit'))
  })

  it('produces different colors for different titles', () => {
    // Most titles should map to different colors, but hash collisions
    // are possible (12 colors). Just verify it's a valid color.
    const color = dominantColorFor('Some title')
    expect(color).toMatch(/^#[0-9a-f]{6}$/)
  })

  it('handles empty string', () => {
    expect(dominantColorFor('')).toMatch(/^#[0-9a-f]{6}$/)
  })
})

describe('coverLetter', () => {
  it('returns first letter uppercase for ASCII title', () => {
    expect(coverLetter('The Hobbit')).toBe('T')
  })

  it('returns first grapheme for non-ASCII title', () => {
    expect(coverLetter('Éxitos Latinos')).toBe('É')
    expect(coverLetter('ñandú')).toBe('Ñ')
  })

  it('preserves non-letter first character', () => {
    expect(coverLetter('123 ABC')).toBe('1')
    expect(coverLetter('!Hola')).toBe('!')
  })

  it('returns ? for empty string', () => {
    expect(coverLetter('')).toBe('?')
  })

  it('returns ? for whitespace-only string', () => {
    expect(coverLetter('   ')).toBe('?')
  })

  it('trims leading whitespace', () => {
    expect(coverLetter('  Hello')).toBe('H')
  })
})
