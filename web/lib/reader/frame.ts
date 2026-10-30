import { Locator } from '@readium/shared'

/**
 * Navigator bookkeeping for the EPUB reader.
 *
 * Extracted from `components/ReaderFrame.svelte`, which drives Readium's
 * navigator through DOM and iframe APIs no test can stand up. The decisions the
 * frame makes about locators — where a stored one points, how far through the
 * book the reader is, whether a `go` actually arrived — are arithmetic on
 * locators, so they live here where they can be exercised directly.
 */

/**
 * A stored locator, or `undefined` when there is none (or it is malformed).
 *
 * The column is JSON written by an earlier version of the reader, so a parse
 * failure reads as "no saved position" rather than stopping the book from
 * opening.
 */
export function parseStoredLocator(saved: string | null | undefined): Locator | undefined {
  if (!saved) return undefined
  try {
    return Locator.deserialize(JSON.parse(saved))
  } catch {
    return undefined
  }
}

/**
 * Number of positions the publication reports for its progress fraction.
 *
 * Falls back to the spine length — a book whose position list has not been
 * generated still has chapters — and then to 1, so the fraction below never
 * divides by zero.
 */
export function publicationTotal(timelinePositions: readonly number[], spineItems: number): number {
  const highest = Math.max(0, ...timelinePositions)
  return highest || spineItems || 1
}

/**
 * Fraction of the whole publication at a locator, clamped to `[0, 1]`.
 *
 * `position` counts from 1, so it is shifted to a zero-based index before the
 * in-resource `progression` is added: a locator naming the second page of the
 * first resource sits at `0 + progression`, not `1 + progression`.
 */
export function bookFraction(
  position: number | undefined,
  progression: number | undefined,
  total: number,
): number {
  if (total <= 0) return 0
  const currentPosition = (position ?? 1) - 1 + (progression ?? 0)
  return Math.min(Math.max(currentPosition / total, 0), 1)
}

/**
 * Whether the navigator has actually arrived at `wanted`.
 *
 * Readium acknowledges `go` against whichever frame is current, so the ack is
 * not proof of arrival. A spine `position` is shared by every page of a
 * reflowable chapter and so cannot prove it alone: `progression` is compared
 * when the target carries one, to a hundredth to absorb the navigator snapping
 * to a column boundary. Fragments are ignored — they name the same resource.
 */
export function arrivalMatches(current: Locator | undefined, wanted: Locator): boolean {
  if (!current) return false
  if (current.href.split('#')[0] !== wanted.href.split('#')[0]) return false

  const wantedPosition = wanted.locations?.position
  if (wantedPosition != null && current.locations?.position !== wantedPosition) return false

  const wantedProgression = wanted.locations?.progression
  if (wantedProgression != null) {
    const currentProgression = current.locations?.progression
    return currentProgression != null && Math.abs(currentProgression - wantedProgression) <= 0.01
  }
  return true
}
