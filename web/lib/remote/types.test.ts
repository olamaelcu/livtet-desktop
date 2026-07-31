import { describe, expect, it } from 'vitest'
import { FAILURE_TOAST, PROVIDER_LABELS, prettyProvider } from './types'

describe('prettyProvider', () => {
  it('returns label for known providers', () => {
    expect(prettyProvider('google_books')).toBe('Google Books')
    expect(prettyProvider('hardcover')).toBe('Hardcover')
    expect(prettyProvider('openlibrary')).toBe('OpenLibrary')
  })

  it('falls back to raw id for unknown providers', () => {
    expect(prettyProvider('unknown_provider')).toBe('unknown_provider')
  })

  it('falls back to empty string for empty id', () => {
    expect(prettyProvider('')).toBe('')
  })
})

describe('PROVIDER_LABELS', () => {
  it('has entries for all known providers', () => {
    expect(PROVIDER_LABELS.google_books).toBe('Google Books')
    expect(PROVIDER_LABELS.hardcover).toBe('Hardcover')
    expect(PROVIDER_LABELS.openlibrary).toBe('OpenLibrary')
  })
})

describe('FAILURE_TOAST', () => {
  it('includes the provider label in the message', () => {
    expect(FAILURE_TOAST('google_books')).toContain('Google Books')
  })

  it('falls back to raw id for unknown providers', () => {
    expect(FAILURE_TOAST('test')).toContain('test')
  })
})
