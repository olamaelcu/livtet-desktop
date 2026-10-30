import { describe, expect, it } from 'vitest'
import { locatorWithQuote } from './anchor'

const locator = JSON.stringify({
  href: '/OEBPS/ch1.xhtml',
  locations: { position: 3, progression: 0.42 },
})

describe('locatorWithQuote', () => {
  it('adds the quote as the locator’s text anchor', () => {
    const merged = JSON.parse(
      locatorWithQuote(locator, { highlight: 'the mailing house', before: 'took the bus to ' }),
    )
    expect(merged.text).toEqual({ highlight: 'the mailing house', before: 'took the bus to ' })
  })

  it('keeps the position the locator carried', () => {
    const merged = JSON.parse(locatorWithQuote(locator, { highlight: 'a phrase' }))
    expect(merged.href).toBe('/OEBPS/ch1.xhtml')
    expect(merged.locations).toEqual({ position: 3, progression: 0.42 })
  })

  it('merges into anchor fields the locator already carries', () => {
    const withText = JSON.stringify({
      href: '/ch1',
      text: { before: 'stale', highlight: 'stale' },
    })
    const merged = JSON.parse(locatorWithQuote(withText, { highlight: 'fresh' }))
    expect(merged.text).toEqual({ before: 'stale', highlight: 'fresh' })
  })

  it('returns the locator untouched when there is no quote to add', () => {
    expect(locatorWithQuote(locator)).toBe(locator)
  })

  // The column is edited by hand in tests and migrations; a note whose locator
  // cannot be parsed must still be writable rather than throwing mid-save.
  it('returns the locator untouched when it is not parseable', () => {
    expect(locatorWithQuote('not json', { highlight: 'a phrase' })).toBe('not json')
  })
})
