/**
 * Human-readable instants.
 *
 * Timestamps arrive from the backend as RFC 3339 (`…Z`), which is unreadable as
 * a wall clock and says nothing about how close the moment is. Everything that
 * shows one to a person wants both: the instant on their own clock, and the
 * distance from now.
 */

/** An instant as a person reads it. */
export interface Moment {
  /** The instant on this machine's clock, e.g. `2 Jan 2026, 04:04`. */
  local: string
  /** Distance from the reference instant, e.g. `in 42 minutes`, `2 days ago`. */
  relative: string
}

// Built once: constructing an `Intl` formatter is expensive and the locale does
// not change while the app runs.
const localFormat = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
})
const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })

/**
 * Unit sizes in milliseconds, largest first.
 *
 * Months and years are the average lengths: an expiry shown to the minute does
 * not need calendar arithmetic, and this keeps a day-wide boundary from
 * flipping between "month" and "day".
 */
const UNITS: readonly (readonly [Intl.RelativeTimeFormatUnit, number])[] = [
  ['year', 365 * 24 * 60 * 60 * 1000],
  ['month', 30 * 24 * 60 * 60 * 1000],
  ['day', 24 * 60 * 60 * 1000],
  ['hour', 60 * 60 * 1000],
  ['minute', 60 * 1000],
]

/**
 * Describe an RFC 3339 instant in local time and relative to `now`.
 *
 * `undefined` when the string is not a timestamp the platform can parse, so the
 * caller can fall back rather than print `Invalid Date`. The relative field is
 * rounded to its unit: `now` once the two instants agree.
 */
export function describeMoment(iso: string, now: Date = new Date()): Moment | undefined {
  const at = new Date(iso)
  const instant = at.getTime()
  if (Number.isNaN(instant)) return undefined

  const delta = instant - now.getTime()
  const [unit, size] = UNITS.find(([, millis]) => Math.abs(delta) >= millis) ?? ['second', 1000]
  const count = Math.round(delta / size)
  return {
    local: localFormat.format(at),
    // `count === 0` also catches the `-0` that rounding a small negative delta
    // produces, which would otherwise read as "0 seconds ago".
    relative: relativeFormat.format(count === 0 ? 0 : count, unit),
  }
}
