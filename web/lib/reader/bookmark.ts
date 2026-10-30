import { commands, type ReaderBookmark } from '$lib/bindings'

/**
 * One page bookmark: a location the reader flagged, with no note text and no
 * highlight colour.
 *
 * The position mirrors {@link ReaderProgress}: a reader family names the unit
 * and the value travels in that unit's field, so bookmarks exist for EPUBs,
 * PDFs, audiobooks, and physical books alike.
 */
export interface Bookmark {
  /** Row id, as lowercase hex; the handle used to remove the bookmark. */
  id: string
  /** Reader family: `'epub'`, `'pdf'`, `'page'` (physical) or `'timestamp'`. */
  kind: string
  /** Serialized Readium locator, for EPUB bookmarks. */
  locator?: string
  /** Bookmarked page, for PDF and physical-book bookmarks. */
  pageNumber?: number
  /** Bookmarked playback position in seconds, for audiobook bookmarks. */
  positionSeconds?: number
  /**
   * Raw stored position, in the unit `kind` names: the fraction of the whole
   * publication for `epub` (not of the chapter), the page number for `pdf` and
   * `page`, the seconds for `timestamp`.
   */
  progress?: number
  /** ISO timestamp of when the bookmark was made. */
  createdAt?: string
}

/** A location to bookmark, in the unit its reader family uses. */
export interface BookmarkPosition {
  kind: 'epub' | 'pdf' | 'page' | 'timestamp'
  /** Serialized Readium locator; required for `epub`. */
  locator?: string
  /** 1-based page; required for `pdf` and `page`. */
  pageNumber?: number
  /** Playback seconds; required for `timestamp`. */
  positionSeconds?: number
  /**
   * Fraction of the whole publication (0-1); required for `epub`. The
   * locator's own `progression` is relative to its chapter, so it cannot be
   * used here.
   */
  progress?: number
}

/**
 * Stable identity for a serialized Readium locator.
 *
 * Neither a spine `position` nor `totalProgression` names the page: both are
 * properties of the resource, so every page of one reflowable chapter shares
 * them (`position: 7, totalProgression: 0.24` while `progression` ran 0.6 →
 * 0.8). Only the in-resource `progression` moves per page, so the key carries
 * it at one part per thousand — fine enough to separate consecutive pages,
 * coarse enough to survive the navigator snapping to a column boundary.
 */
export function locatorKey(locatorJson: string | undefined): string {
  if (!locatorJson) return ''
  try {
    const parsed = JSON.parse(locatorJson) as {
      href?: string
      locations?: { position?: number; progression?: number; totalProgression?: number }
    }
    const locations = parsed.locations ?? {}
    const fraction = locations.progression ?? locations.totalProgression
    const anchor = fraction === undefined ? (locations.position ?? '') : fraction.toFixed(3)
    return `epub:${parsed.href ?? ''}#${anchor}`
  } catch {
    // An unparsable locator is still a location the reader can revisit.
    return `epub:${locatorJson}`
  }
}

/**
 * Stable identity for a bookmarked position, so visiting the same place twice
 * toggles the bookmark off instead of stacking duplicates.
 */
export function bookmarkKey(bookmark: Bookmark): string {
  if (bookmark.pageNumber !== undefined) return `page:${bookmark.pageNumber}`
  if (bookmark.positionSeconds !== undefined) {
    return `time:${Math.round(bookmark.positionSeconds)}`
  }
  return locatorKey(bookmark.locator)
}

/** Whether `position` is already bookmarked. */
export function isBookmarked(bookmarks: Bookmark[], position: BookmarkPosition): boolean {
  const key =
    position.kind === 'epub'
      ? locatorKey(position.locator)
      : position.pageNumber !== undefined
        ? `page:${position.pageNumber}`
        : `time:${Math.round(position.positionSeconds ?? 0)}`
  if (!key) return false
  return bookmarks.some((bookmark) => bookmarkKey(bookmark) === key)
}

/**
 * `bookmarks` with `added` appended, or unchanged when the save failed.
 *
 * `addBookmark` reports failure as `undefined`, and every reader treats a
 * failed save as "nothing happened" rather than as an error the reader must
 * dismiss. Order is the stored order, which is also the order the lists render.
 */
export function withBookmark(bookmarks: Bookmark[], added: Bookmark | undefined): Bookmark[] {
  return added ? [...bookmarks, added] : bookmarks
}

/** `bookmarks` without the one carrying `id`; order preserved. */
export function withoutBookmark(bookmarks: Bookmark[], id: string): Bookmark[] {
  return bookmarks.filter((bookmark) => bookmark.id !== id)
}

/** Human-readable place in the book: "Page 12", "42:07", "25.6% through". */
export function bookmarkLabel(bookmark: Bookmark): string {
  if (bookmark.pageNumber !== undefined) return `Page ${bookmark.pageNumber}`
  if (bookmark.positionSeconds !== undefined) {
    const total = Math.max(0, Math.round(bookmark.positionSeconds))
    const minutes = Math.floor(total / 60)
    const seconds = total % 60
    return `${minutes}:${seconds.toString().padStart(2, '0')}`
  }
  // The recorded fraction is the page's place in the whole book. Rows written
  // before it was recorded fall back to the locator's chapter-relative
  // progression rather than printing nothing.
  const fraction = bookmark.progress ?? locatorProgression(bookmark.locator)
  if (fraction === undefined) return 'Bookmarked location'
  // One decimal: consecutive pages can be under a percent apart, and a
  // whole-percent label would print them identically.
  return `${Math.round(fraction * 1000) / 10}% through`
}

/** The locator's reading fraction, when it carries one. */
function locatorProgression(locatorJson: string | undefined): number | undefined {
  if (!locatorJson) return undefined
  try {
    const parsed = JSON.parse(locatorJson) as {
      locations?: { progression?: number; totalProgression?: number }
    }
    const progression = parsed.locations?.progression ?? parsed.locations?.totalProgression
    return typeof progression === 'number' ? progression : undefined
  } catch {
    return undefined
  }
}

/** Map a backend row onto the reader-facing shape. */
function toBookmark(row: ReaderBookmark): Bookmark {
  const createdAt = new Date((row.created_at ?? 0) * 1000)
  return {
    id: row.id,
    kind: row.kind,
    locator: row.locator ?? undefined,
    pageNumber: row.page_number ?? undefined,
    positionSeconds: row.position_seconds ?? undefined,
    progress: row.progress ?? undefined,
    createdAt: Number.isNaN(createdAt.getTime()) ? undefined : createdAt.toISOString(),
  }
}

/**
 * Stored bookmarks for an edition, oldest first.
 *
 * Failures read as "no bookmarks": a transient IPC/database error must not
 * break the reader, and the user can bookmark again.
 */
export async function loadBookmarks(editionId: string): Promise<Bookmark[]> {
  try {
    const result = await commands.listReaderBookmarks(editionId)
    if (result.status !== 'ok') return []
    return result.data.map(toBookmark)
  } catch {
    return []
  }
}

/** Bookmark `position`. Returns the stored bookmark, or `undefined` on failure. */
export async function addBookmark(
  editionId: string,
  position: BookmarkPosition,
): Promise<Bookmark | undefined> {
  try {
    const result = await commands.addReaderBookmark(
      editionId,
      position.kind,
      position.locator ?? null,
      position.pageNumber ?? null,
      position.positionSeconds ?? null,
      position.progress ?? null,
    )
    return result.status === 'ok' ? toBookmark(result.data) : undefined
  } catch {
    return undefined
  }
}

/** Remove a bookmark by id. A bookmark that is already gone is not an error. */
export async function removeBookmark(bookmarkId: string): Promise<boolean> {
  try {
    const result = await commands.removeReaderBookmark(bookmarkId)
    return result.status === 'ok'
  } catch {
    return false
  }
}
