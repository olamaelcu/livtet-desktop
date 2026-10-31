import { describe, expect, it } from 'vitest'
import { describeMoment } from './datetime'

const BASE = '2026-01-02T03:04:05.000Z'
const base = new Date(BASE)

/** `now` shifted from `BASE` by `millis`, in the direction of the future. */
function shifted(millis: number): Date {
  return new Date(base.getTime() + millis)
}

describe('describeMoment', () => {
  it('reads a near-future instant in minutes', () => {
    expect(describeMoment('2026-01-02T03:46:05.000Z', base)?.relative).toBe('in 42 minutes')
  })

  it('reads a past instant as elapsed time', () => {
    expect(describeMoment('2026-01-02T02:59:05.000Z', base)?.relative).toBe('5 minutes ago')
  })

  it('reads seconds when the moment is under a minute away', () => {
    expect(describeMoment('2026-01-02T03:04:35.000Z', base)?.relative).toBe('in 30 seconds')
  })

  it('reads "now" once the instants agree', () => {
    expect(describeMoment(BASE, base)?.relative).toBe('now')
    // Rounding a sub-unit delta can produce -0, which must not read as "ago".
    expect(describeMoment(BASE, shifted(500))?.relative).toBe('now')
  })

  it('moves up the units as the distance grows', () => {
    expect(describeMoment('2026-01-02T06:04:05.000Z', base)?.relative).toBe('in 3 hours')
    expect(describeMoment('2026-01-05T03:04:05.000Z', base)?.relative).toBe('in 3 days')
    expect(describeMoment('2026-03-03T03:04:05.000Z', base)?.relative).toBe('in 2 months')
  })
  // `numeric: 'auto'` is deliberate: one whole unit away reads as "tomorrow"
  // rather than "in 1 day", the way a person would say it.
  it('uses the natural wording a whole unit away', () => {
    expect(describeMoment('2026-01-03T03:04:05.000Z', base)?.relative).toBe('tomorrow')
    expect(describeMoment('2026-01-01T03:04:05.000Z', base)?.relative).toBe('yesterday')
    expect(describeMoment('2027-02-02T03:04:05.000Z', base)?.relative).toBe('next year')
  })

  // The stored value is UTC; what a reader needs is their own clock. The check
  // compares against the platform formatting the same instant locally, and
  // against the UTC rendering whenever the machine is not on UTC.
  it('shows the instant on this machine clock', () => {
    const iso = '2026-01-02T23:30:00.000Z'
    const instant = new Date(iso)
    const moment = describeMoment(iso, instant)
    const options = { dateStyle: 'medium', timeStyle: 'short' } as const

    expect(moment?.local).toBe(new Intl.DateTimeFormat(undefined, options).format(instant))
    if (instant.getTimezoneOffset() !== 0) {
      expect(moment?.local).not.toBe(
        new Intl.DateTimeFormat(undefined, { ...options, timeZone: 'UTC' }).format(instant),
      )
    }
  })

  // A malformed column must not surface as "Invalid Date" in the panel.
  it('reports nothing for a string that is not a timestamp', () => {
    expect(describeMoment('', base)).toBeUndefined()
    expect(describeMoment('not a date', base)).toBeUndefined()
    expect(describeMoment('2026-13-45T99:99:99Z', base)).toBeUndefined()
  })
})
