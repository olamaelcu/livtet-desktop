import { describe, expect, it } from 'vitest'
import { clampSeek, needsPersist, RESUME_FLOOR_SECONDS, resumePoint } from './playback'

describe('clampSeek', () => {
  it('keeps a target inside the track', () => {
    expect(clampSeek(30, 200)).toBe(30)
  })

  it('pulls a negative target back to the start', () => {
    expect(clampSeek(-5, 200)).toBe(0)
  })

  it('pulls a target past the end back to the last instant', () => {
    expect(clampSeek(500, 200)).toBe(200)
  })

  // Before metadata loads the element reports NaN and the caller falls back to
  // a duration of 0; a seek then lands on the start rather than nowhere.
  it('clamps to the start when the duration is not known', () => {
    expect(clampSeek(90, 0)).toBe(0)
  })
})

describe('needsPersist', () => {
  it('always writes a forced save', () => {
    expect(needsPersist(10, 10, true)).toBe(true)
  })

  it('skips a write when the position has not moved a second', () => {
    expect(needsPersist(10.6, 10)).toBe(false)
  })

  it('writes once the position has moved a second', () => {
    expect(needsPersist(11, 10)).toBe(true)
    expect(needsPersist(9, 10)).toBe(true)
  })
})

describe('resumePoint', () => {
  it('resumes a position past the opening seconds', () => {
    expect(resumePoint(120, 3600)).toBe(120)
  })

  it('does not resume a save made at the very start', () => {
    expect(resumePoint(RESUME_FLOOR_SECONDS, 3600)).toBeUndefined()
    expect(resumePoint(0, 3600)).toBeUndefined()
  })

  it('does not resume at or past the end of the recording', () => {
    expect(resumePoint(3600, 3600)).toBeUndefined()
    expect(resumePoint(4000, 3600)).toBeUndefined()
  })

  it('does not resume before the length is known', () => {
    expect(resumePoint(120, 0)).toBeUndefined()
  })

  it('does not resume when nothing was ever stored', () => {
    expect(resumePoint(null, 3600)).toBeUndefined()
    expect(resumePoint(undefined, 3600)).toBeUndefined()
  })
})
