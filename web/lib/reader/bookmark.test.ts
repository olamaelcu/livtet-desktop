import { beforeEach, describe, expect, it, vi } from 'vitest'

// The bookmark store talks to the generated bindings; stub the commands so
// tests stay offline and can assert exactly what was sent.
const listReaderBookmarks = vi.fn()
const addReaderBookmark = vi.fn()
const removeReaderBookmark = vi.fn()
vi.mock('$lib/bindings', () => ({
  commands: {
    listReaderBookmarks: (...args: unknown[]) => listReaderBookmarks(...args),
    addReaderBookmark: (...args: unknown[]) => addReaderBookmark(...args),
    removeReaderBookmark: (...args: unknown[]) => removeReaderBookmark(...args),
  },
}))

import {
  addBookmark,
  bookmarkKey,
  bookmarkLabel,
  isBookmarked,
  loadBookmarks,
  locatorKey,
  removeBookmark,
} from './bookmark'

const editionId = '01ABCDEF'

describe('bookmark positions', () => {
  it('keys a locator by the in-resource progression, not by resource-wide numbers', () => {
    // Two pages of one chapter share both `position` and `totalProgression`
    // (0.24 for the whole chapter in a real Readium locator); only
    // `progression` moves. They must still be distinct bookmarks.
    const pageOne = locatorKey(
      '{"href":"preface","locations":{"position":7,"totalProgression":0.24,"progression":0.6}}',
    )
    const pageTwo = locatorKey(
      '{"href":"preface","locations":{"position":7,"totalProgression":0.24,"progression":0.8}}',
    )
    expect(pageOne).toBe('epub:preface#0.600')
    expect(pageTwo).toBe('epub:preface#0.800')
    expect(pageOne).not.toBe(pageTwo)

    // Scrolling inside the page only moves `progression` by less than a
    // thousandth: the page keeps its key.
    expect(locatorKey('{"href":"preface","locations":{"progression":0.6004}}')).toBe(
      locatorKey('{"href":"preface","locations":{"progression":0.6}}'),
    )

    // A locator carrying only `totalProgression` still gets an anchor.
    expect(locatorKey('{"href":"ch2","locations":{"totalProgression":0.2561}}')).toBe(
      'epub:ch2#0.256',
    )
    // Without any fraction, the spine position is the only anchor left.
    expect(locatorKey('{"href":"ch3","locations":{"position":4}}')).toBe('epub:ch3#4')
    expect(locatorKey(undefined)).toBe('')
    expect(locatorKey('not json')).toBe('epub:not json')
  })

  it('matches a position to its bookmark across reader families', () => {
    const page = { id: 'p', kind: 'pdf', pageNumber: 7 }
    const seconds = { id: 's', kind: 'timestamp', positionSeconds: 900.4 }
    const epub = { id: 'e', kind: 'epub', locator: '{"href":"ch1","locations":{"position":3}}' }
    // A second page of the same chapter is a different bookmark.
    const nextPage = {
      id: 'e2',
      kind: 'epub',
      locator: '{"href":"ch1","locations":{"position":3,"progression":0.5}}',
    }
    const bookmarks = [page, seconds, epub]

    expect(bookmarkKey(page)).toBe('page:7')
    expect(bookmarkKey(seconds)).toBe('time:900')
    expect(isBookmarked(bookmarks, { kind: 'pdf', pageNumber: 7 })).toBe(true)
    // PDF and physical page bookmarks share one key: an edition has one format.
    expect(isBookmarked(bookmarks, { kind: 'page', pageNumber: 7 })).toBe(true)
    expect(isBookmarked(bookmarks, { kind: 'timestamp', positionSeconds: 900 })).toBe(true)
    expect(
      isBookmarked(bookmarks, {
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"position":3}}',
      }),
    ).toBe(true)
    expect(
      isBookmarked([...bookmarks, nextPage], {
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"position":3,"progression":0.5}}',
      }),
    ).toBe(true)
    expect(
      isBookmarked(bookmarks, {
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"position":4}}',
      }),
    ).toBe(false)
    // Same chapter, different page: not the bookmarked one.
    expect(
      isBookmarked(bookmarks, {
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"position":3,"progression":0.2}}',
      }),
    ).toBe(false)
  })

  it('labels page, audiobook and EPUB bookmarks', () => {
    expect(bookmarkLabel({ id: 'p', kind: 'pdf', pageNumber: 12 })).toBe('Page 12')
    expect(bookmarkLabel({ id: 's', kind: 'timestamp', positionSeconds: 2527 })).toBe('42:07')
    // An EPUB label is the page's place in the whole book, not in its chapter:
    // the recorded fraction wins over the locator's chapter-relative one.
    expect(
      bookmarkLabel({
        id: 'e',
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"progression":0.8}}',
        progress: 0.256,
      }),
    ).toBe('25.6% through')
    // Consecutive pages stay distinguishable in the list.
    expect(bookmarkLabel({ id: 'e', kind: 'epub', progress: 0.264 })).toBe('26.4% through')
    // Rows written before the book fraction was recorded fall back to the
    // locator rather than printing nothing.
    expect(
      bookmarkLabel({
        id: 'e',
        kind: 'epub',
        locator: '{"href":"ch1","locations":{"progression":0.314}}',
      }),
    ).toBe('31.4% through')
    expect(bookmarkLabel({ id: 'e', kind: 'epub' })).toBe('Bookmarked location')
  })
})

describe('bookmark store', () => {
  beforeEach(() => {
    listReaderBookmarks.mockReset()
    listReaderBookmarks.mockResolvedValue({ status: 'ok', data: [] })
    addReaderBookmark.mockReset()
    addReaderBookmark.mockResolvedValue({ status: 'ok', data: null })
    removeReaderBookmark.mockReset()
    removeReaderBookmark.mockResolvedValue({ status: 'ok', data: true })
  })

  it('maps stored rows onto bookmark fields', async () => {
    listReaderBookmarks.mockResolvedValue({
      status: 'ok',
      data: [
        {
          id: 'ab',
          kind: 'pdf',
          locator: null,
          page_number: 4,
          position_seconds: null,
          progress: 4,
          created_at: 1_774_000_000,
        },
      ],
    })

    const bookmarks = await loadBookmarks(editionId)

    expect(listReaderBookmarks).toHaveBeenCalledWith(editionId)
    expect(bookmarks).toEqual([
      {
        id: 'ab',
        kind: 'pdf',
        locator: undefined,
        pageNumber: 4,
        positionSeconds: undefined,
        progress: 4,
        createdAt: new Date(1_774_000_000_000).toISOString(),
      },
    ])
  })

  it('sends the position in its reader family unit', async () => {
    await addBookmark(editionId, {
      kind: 'epub',
      locator: '{"href":"ch1"}',
      progress: 0.256,
    })
    expect(addReaderBookmark).toHaveBeenCalledWith(
      editionId,
      'epub',
      '{"href":"ch1"}',
      null,
      null,
      0.256,
    )

    addReaderBookmark.mockResolvedValue({
      status: 'ok',
      data: {
        id: 'cd',
        kind: 'timestamp',
        locator: null,
        page_number: null,
        position_seconds: 90,
        progress: 90,
        created_at: 1_774_000_000,
      },
    })
    const stored = await addBookmark(editionId, { kind: 'timestamp', positionSeconds: 90 })
    expect(addReaderBookmark).toHaveBeenLastCalledWith(editionId, 'timestamp', null, null, 90, null)
    expect(stored?.positionSeconds).toBe(90)
  })

  it('treats a failed load as no bookmarks and reports removal failures', async () => {
    listReaderBookmarks.mockResolvedValue({ status: 'error', error: { code: 'database' } })
    expect(await loadBookmarks(editionId)).toEqual([])

    removeReaderBookmark.mockRejectedValue(new Error('offline'))
    expect(await removeBookmark('ab')).toBe(false)
  })
})
