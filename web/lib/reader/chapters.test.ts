import { describe, expect, it } from 'vitest'
import { chapterHref, chaptersFrom } from './chapters'

const XHTML = 'application/xhtml+xml'

describe('chaptersFrom', () => {
  it('titles each spine resource from the table of contents', () => {
    const chapters = chaptersFrom(
      [
        { href: 'OEBPS/cover.xhtml', type: XHTML },
        { href: 'OEBPS/ch01.xhtml', type: XHTML },
      ],
      [
        { href: 'OEBPS/cover.xhtml', title: 'Cover' },
        { href: 'OEBPS/ch01.xhtml', title: '  The Long Way  ' },
      ],
    )

    expect(chapters).toEqual([
      { href: 'OEBPS/cover.xhtml', type: XHTML, title: 'Cover' },
      { href: 'OEBPS/ch01.xhtml', type: XHTML, title: 'The Long Way' },
    ])
  })

  // Reading-order links carry no title of their own: the list has to be built
  // from the spine, so a book whose toc the streamer could not extract still
  // navigates.
  it('keeps spine resources the table of contents does not name', () => {
    const chapters = chaptersFrom(
      [
        { href: 'OEBPS/ch02.xhtml', type: XHTML },
        { href: 'OEBPS/ch03.xhtml', type: XHTML },
      ],
      [{ href: 'OEBPS/ch01.xhtml', title: 'One' }],
    )

    expect(chapters.map((chapter) => chapter.title)).toEqual(['ch02', 'ch03'])
  })

  it('names an unnamed resource by its place in the list when the href has no stem', () => {
    const chapters = chaptersFrom([{ href: 'OEBPS/.xhtml', type: XHTML }], [])

    expect(chapters[0]?.title).toBe('Chapter 1')
  })

  // The drawer builds a locator from each chapter, and a locator without a media
  // type is incomplete.
  it('fills in the media type a link omits', () => {
    const chapters = chaptersFrom([{ href: 'a.xhtml' }, { href: 'b.xhtml' }], [])

    expect(chapters.every((chapter) => chapter.type === XHTML)).toBe(true)
  })

  // A toc entry that targets a place inside a resource still names that
  // resource; the fragment is not part of the address the frame navigates to.
  it('joins a fragmented toc entry to its resource', () => {
    const chapters = chaptersFrom(
      [{ href: 'OEBPS/text.xhtml#part-2', type: XHTML }],
      [{ href: 'OEBPS/text.xhtml#part-2', title: 'Part Two' }],
    )

    expect(chapters).toEqual([{ href: 'OEBPS/text.xhtml', type: XHTML, title: 'Part Two' }])
  })

  it('lists a resource the spine repeats once', () => {
    const chapters = chaptersFrom(
      [
        { href: 'OEBPS/a.xhtml', type: XHTML },
        { href: 'OEBPS/a.xhtml#again', type: XHTML },
      ],
      [],
    )

    expect(chapters).toHaveLength(1)
    expect(chapters[0]?.href).toBe('OEBPS/a.xhtml')
  })

  it('lists no chapters without a reading order', () => {
    expect(chaptersFrom(undefined, [{ href: 'a.xhtml', title: 'A' }])).toEqual([])
    expect(chaptersFrom([], [])).toEqual([])
  })
})

describe('chapterHref', () => {
  it('reads the resource from the published locator', () => {
    expect(
      chapterHref(JSON.stringify({ href: 'OEBPS/ch04.xhtml#s3', type: XHTML, locations: {} })),
    ).toBe('OEBPS/ch04.xhtml')
  })

  // The published locator is empty until the navigator reports a position, and
  // a malformed one must not mark a row current.
  it('reports no resource for an empty or unparseable locator', () => {
    expect(chapterHref('')).toBe('')
    expect(chapterHref('{"href":')).toBe('')
    expect(chapterHref('null')).toBe('')
    expect(chapterHref(JSON.stringify({ type: XHTML }))).toBe('')
    expect(chapterHref(JSON.stringify({ href: 4 }))).toBe('')
  })
})
