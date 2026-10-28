import { describe, expect, it } from 'vitest'
import { DEFAULT_FONT_SIZE, stepFontSize } from './fontSize'

describe('stepFontSize', () => {
  it('steps by the increment Readium’s preference editor uses', () => {
    expect(stepFontSize(DEFAULT_FONT_SIZE, 1)).toBe(1.05)
    expect(stepFontSize(DEFAULT_FONT_SIZE, -1)).toBe(0.95)
  })

  it('stops at the ends of the range Readium accepts', () => {
    expect(stepFontSize(3.99, 1)).toBe(4)
    expect(stepFontSize(0.71, -1)).toBe(0.7)
    expect(stepFontSize(4, 1)).toBe(4)
    expect(stepFontSize(0.7, -1)).toBe(0.7)
  })

  it('rounds away floating point drift', () => {
    // 1.05 - 0.05 must be 1, not 0.9999999999999999, which renders a hair off.
    expect(String(stepFontSize(1.05, -1))).toBe('1')
  })
})
