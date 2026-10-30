import { Locator } from '@readium/shared'
import { describe, expect, it } from 'vitest'
import { arrivalMatches, bookFraction, parseStoredLocator, publicationTotal } from './frame'

const XHTML = 'application/xhtml+xml'

/** A locator as the navigator reports one, with the fields the reader reads. */
function locator(
  href: string,
  locations?: { position?: number; progression?: number; totalProgression?: number },
): Locator {
  return Locator.deserialize({ href, type: XHTML, locations }) as Locator
}

describe('parseStoredLocator', () => {
  it('reads a locator back from the stored JSON', () => {
    const parsed = parseStoredLocator(
      JSON.stringify({ href: '/ch2.xhtml', type: XHTML, locations: { position: 4 } }),
    )
    expect(parsed?.href).toBe('/ch2.xhtml')
    expect(parsed?.locations?.position).toBe(4)
  })

  it('reports no position when the column is empty', () => {
    expect(parseStoredLocator('')).toBeUndefined()
    expect(parseStoredLocator(null)).toBeUndefined()
    expect(parseStoredLocator(undefined)).toBeUndefined()
  })

  // The column is JSON written by an earlier build; a book must still open when
  // it does not parse.
  it('reports no position when the column is not parseable', () => {
    expect(parseStoredLocator('{"href":')).toBeUndefined()
    expect(parseStoredLocator('null')).toBeUndefined()
    expect(parseStoredLocator('{"type":"application/xhtml+xml"}')).toBeUndefined()
  })
})

describe('publicationTotal', () => {
  it('uses the highest position the timeline reports', () => {
    expect(publicationTotal([1, 2, 3, 9, 4], 3)).toBe(9)
  })

  it('falls back to the spine length when no positions were generated', () => {
    expect(publicationTotal([], 24)).toBe(24)
  })

  it('falls back to 1 so the fraction can never divide by zero', () => {
    expect(publicationTotal([], 0)).toBe(1)
  })
})

describe('bookFraction', () => {
  it('counts a position from 1 as a zero-based index', () => {
    // Page 1 of 10 is the start, not a tenth of the way in.
    expect(bookFraction(1, 0, 10)).toBe(0)
    expect(bookFraction(2, 0, 10)).toBeCloseTo(0.1)
  })

  it('adds the in-resource progression to the position', () => {
    expect(bookFraction(3, 0.5, 10)).toBeCloseTo(0.25)
  })

  it('treats a missing position as the first page', () => {
    expect(bookFraction(undefined, 0, 10)).toBe(0)
  })

  it('clamps a locator that names more than the publication holds', () => {
    expect(bookFraction(20, 0, 10)).toBe(1)
    expect(bookFraction(1, -2, 10)).toBe(0)
  })

  it('reports zero progress when no total is known', () => {
    expect(bookFraction(3, 0.5, 0)).toBe(0)
  })
})

describe('arrivalMatches', () => {
  it('matches the same position in the same resource', () => {
    expect(arrivalMatches(locator('/ch1', { position: 3 }), locator('/ch1', { position: 3 }))).toBe(
      true,
    )
  })

  it('is false when the navigator still shows another resource', () => {
    expect(arrivalMatches(locator('/ch1', { position: 3 }), locator('/ch2', { position: 3 }))).toBe(
      false,
    )
  })

  it('is false before the navigator reports a locator', () => {
    expect(arrivalMatches(undefined, locator('/ch1'))).toBe(false)
  })

  // A chapter's pages share one spine position, so a bookmark inside the
  // current chapter would "arrive" without the reader moving at all.
  it('compares progression when the target carries one', () => {
    const wanted = locator('/ch1', { position: 7, progression: 0.8 })
    expect(arrivalMatches(locator('/ch1', { position: 7, progression: 0.8 }), wanted)).toBe(true)
    expect(arrivalMatches(locator('/ch1', { position: 7, progression: 0.3 }), wanted)).toBe(false)
  })

  it('absorbs the navigator snapping to a column boundary', () => {
    const wanted = locator('/ch1', { progression: 0.5 })
    expect(arrivalMatches(locator('/ch1', { progression: 0.505 }), wanted)).toBe(true)
    expect(arrivalMatches(locator('/ch1', { progression: 0.52 }), wanted)).toBe(false)
  })

  it('never matches on a position alone when it differs', () => {
    expect(arrivalMatches(locator('/ch1', { position: 4 }), locator('/ch1', { position: 5 }))).toBe(
      false,
    )
  })

  it('ignores fragments, which name the same resource', () => {
    expect(
      arrivalMatches(locator('/ch1#p3', { position: 3 }), locator('/ch1', { position: 3 })),
    ).toBe(true)
  })
})
