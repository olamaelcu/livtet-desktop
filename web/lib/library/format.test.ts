import { describe, expect, it } from 'vitest'
import { fileName, formatFileSize, formatIdentifier } from './format'

describe('formatFileSize', () => {
  it('renders bytes without a unit divisor', () => {
    expect(formatFileSize(512)).toBe('512 B')
  })

  it('scales to the largest fitting unit', () => {
    expect(formatFileSize(1024)).toBe('1.0 KB')
    expect(formatFileSize(1024 * 1024 * 1.5)).toBe('1.5 MB')
    expect(formatFileSize(1024 * 1024 * 20)).toBe('20 MB')
  })

  it('guards invalid input', () => {
    expect(formatFileSize(-1)).toBe('—')
    expect(formatFileSize(Number.NaN)).toBe('—')
  })
})

describe('fileName', () => {
  it('returns the trailing segment for posix and windows paths', () => {
    expect(fileName('/a/b/book.epub')).toBe('book.epub')
    expect(fileName('C:\\a\\book.epub')).toBe('book.epub')
  })
})

describe('formatIdentifier', () => {
  it('strips urn prefixes to the bare value', () => {
    expect(formatIdentifier('urn:isbn:9780306406157')).toBe('9780306406157')
    expect(formatIdentifier('urn:uuid:372c43e0-812a-4ab2-8076-84c7b1c474af')).toBe(
      '372c43e0-812a-4ab2-8076-84c7b1c474af',
    )
  })

  it('matches the urn scheme case-insensitively', () => {
    expect(formatIdentifier('URN:UUID:372c43e0-812a-4ab2-8076-84c7b1c474af')).toBe(
      '372c43e0-812a-4ab2-8076-84c7b1c474af',
    )
  })

  it('leaves bare identifiers unchanged', () => {
    expect(formatIdentifier('372c43e0-812a-4ab2-8076-84c7b1c474af')).toBe(
      '372c43e0-812a-4ab2-8076-84c7b1c474af',
    )
    expect(formatIdentifier('B0CR977BQH')).toBe('B0CR977BQH')
    expect(formatIdentifier('example.com/books/1')).toBe('example.com/books/1')
  })

  it('leaves a prefix with an empty value unchanged', () => {
    expect(formatIdentifier('urn:isbn:')).toBe('urn:isbn:')
  })
})
