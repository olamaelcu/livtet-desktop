import { describe, expect, it } from 'vitest'
import { createLoadSession } from './loadSession'

describe('createLoadSession', () => {
  it('hands out increasing generations that stay current until superseded', () => {
    const session = createLoadSession()
    const first = session.begin()
    const second = session.begin()
    expect(second).toBeGreaterThan(first)
    expect(session.isCurrent(first)).toBe(false)
    expect(session.isCurrent(second)).toBe(true)
  })

  it('retires the current generation on invalidate', () => {
    const session = createLoadSession()
    const generation = session.begin()
    session.invalidate()
    expect(session.isCurrent(generation)).toBe(false)
  })
})
