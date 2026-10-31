import { describe, expect, it } from 'vitest'
import {
  composeIdentifier,
  describeCatalogError,
  fileName,
  formatFileSize,
  formatIdentifier,
} from './format'

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

describe('composeIdentifier', () => {
  it('rebuilds the stored urn under the edited kind', () => {
    expect(composeIdentifier('oclc', '12345', 'urn:isbn:9780306406157')).toBe('urn:oclc:12345')
  })

  it('round-trips the display form of a stored urn', () => {
    const stored = 'urn:isbn:9780306406157'
    expect(composeIdentifier('isbn', formatIdentifier(stored), stored)).toBe(stored)
    const web = 'urn:http:https://example.com/books/1'
    expect(composeIdentifier('http', formatIdentifier(web), web)).toBe(web)
  })

  it('keeps a value typed out in full verbatim', () => {
    expect(composeIdentifier('isbn', ' urn:oclc:12345 ', 'urn:isbn:9780306406157')).toBe(
      'urn:oclc:12345',
    )
  })

  it('adds no prefix to an entry that was never a urn', () => {
    expect(composeIdentifier('other', ' abc ', 'abc')).toBe('abc')
  })

  it('trims the kind it writes', () => {
    expect(composeIdentifier(' isbn ', ' 978 ', 'urn:isbn:1')).toBe('urn:isbn:978')
  })
})

describe('describeCatalogError', () => {
  it('reports the id of an unparseable edition', () => {
    expect(describeCatalogError({ InvalidId: { id: 'nope' } })).toBe('Invalid edition id: nope')
  })

  it('passes a database failure through', () => {
    expect(describeCatalogError({ Database: { message: 'disk is full' } })).toBe('disk is full')
  })

  it('reports a rejected field and an entry that moved', () => {
    expect(describeCatalogError({ InvalidIdentifier: { message: 'kind is blank' } })).toBe(
      'kind is blank',
    )
    expect(
      describeCatalogError({
        IdentifierNotFound: { edition_id: 'e', kind: 'isbn', value: 'urn:isbn:1' },
      }),
    ).toBe('That identifier is no longer on this book')
  })

  it('explains a kind clash on a shared value', () => {
    expect(
      describeCatalogError({
        IdentifierConflict: { value: 'urn:oclc:1', existing_kind: 'oclc', requested_kind: 'lccn' },
      }),
    ).toBe('Value urn:oclc:1 is already stored as oclc')
  })
})
