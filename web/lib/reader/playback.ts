/**
 * Playback arithmetic for the audiobook reader.
 *
 * Extracted from `routes/reader/audio/[editionId]/+page.svelte` so the rules
 * that decide what gets saved, and where playback resumes, are testable without
 * an `<audio>` element: the element only supplies `currentTime` and `duration`.
 */

/** Seconds of playback between automatic progress saves. */
export const SAVE_EVERY_SECONDS = 15

/** A saved position at or below this many seconds counts as "not started". */
export const RESUME_FLOOR_SECONDS = 5

/** Smallest move, in seconds, that is worth a write. */
const PERSIST_EPSILON = 1

/**
 * A seek target clamped into the track.
 *
 * Callers resolve `duration` as `audio.duration || duration` first: the element
 * reports `NaN` until metadata loads, and `NaN` propagates through both
 * comparisons below. An unknown duration is therefore 0, which clamps the seek
 * to the start rather than leaving playback wherever it was.
 */
export function clampSeek(target: number, duration: number): number {
  return Math.min(Math.max(0, target), duration)
}

/**
 * Whether to write the position now.
 *
 * Forced saves — pause, end of track, window hidden — always write. Otherwise
 * the position must have moved by at least a second, so an idle reader that
 * merely re-renders does not churn the database.
 */
export function needsPersist(position: number, lastSaved: number, force = false): boolean {
  return force || Math.abs(position - lastSaved) >= PERSIST_EPSILON
}

/**
 * Where to resume, or `undefined` to start from the beginning.
 *
 * A save in the first few seconds is an artefact of opening the book, and one
 * at or past the end would drop the listener on the closing words. Resuming is
 * only meaningful once the recording's length is known, so `total` of 0 resumes
 * nowhere.
 */
export function resumePoint(saved: number | null | undefined, total: number): number | undefined {
  if (saved === null || saved === undefined) return undefined
  if (saved > RESUME_FLOOR_SECONDS && saved < total) return saved
  return undefined
}
