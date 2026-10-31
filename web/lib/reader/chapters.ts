/**
 * Chapter navigation for the EPUB reader.
 *
 * A publication's spine is its chapter list: one resource per chapter, in
 * reading order. Titles come from the table of contents instead, which the
 * streamer emits as a separate `toc` collection — reading-order links carry an
 * href and a media type and nothing else — so the two are joined here by
 * resource.
 */

/** One spine resource, named for the chapter drawer. */
export interface Chapter {
  /** Resource href, archive-root-relative as every publication link carries it. */
  href: string
  /** Media type of the resource, so a locator built from it is complete. */
  type: string
  /** Table-of-contents title, or a stand-in derived from the resource name. */
  title: string
}

/** The fields of a Readium `Link` the chapter list reads. */
export interface ChapterLink {
  href: string
  type?: string
  title?: string
}

/** Spine resources are documents; a link that omits its type is one of these. */
const DEFAULT_TYPE = 'application/xhtml+xml'

/**
 * The publication's chapters: its spine, titled from its table of contents.
 *
 * Hrefs are compared without their fragment, because a table-of-contents entry
 * pointing at a place inside a resource still names that resource's chapter;
 * the first such entry wins. Every spine resource stays in the list whether the
 * table of contents names it or not — the drawer's job is to reach any chapter
 * — and one it does not name keeps its file stem rather than an empty row.
 */
export function chaptersFrom(
  readingOrder: readonly ChapterLink[] | undefined,
  toc: readonly ChapterLink[] | undefined,
): Chapter[] {
  const titles = new Map<string, string>()
  for (const entry of toc ?? []) {
    const href = entry.href.split('#')[0] ?? ''
    const title = entry.title?.trim()
    if (href !== '' && title && !titles.has(href)) titles.set(href, title)
  }

  const chapters: Chapter[] = []
  const seen = new Set<string>()
  for (const item of readingOrder ?? []) {
    const href = item.href.split('#')[0] ?? ''
    // A spine that names one resource twice is still one chapter to navigate to.
    if (href === '' || seen.has(href)) continue
    seen.add(href)
    const name = href.slice(href.lastIndexOf('/') + 1)
    const stem = (name.split('.')[0] ?? '').trim()
    chapters.push({
      href,
      type: item.type ?? DEFAULT_TYPE,
      title: titles.get(href) || item.title?.trim() || stem || `Chapter ${chapters.length + 1}`,
    })
  }
  return chapters
}

/**
 * The resource on screen, read from the serialized locator the frame publishes.
 *
 * Empty until the navigator reports a position, and for a locator that does not
 * parse: no row is marked current rather than the wrong one.
 */
export function chapterHref(location: string): string {
  if (location === '') return ''
  let parsed: unknown
  try {
    parsed = JSON.parse(location)
  } catch {
    return ''
  }
  if (typeof parsed !== 'object' || parsed === null || !('href' in parsed)) return ''
  return typeof parsed.href === 'string' ? (parsed.href.split('#')[0] ?? '') : ''
}
