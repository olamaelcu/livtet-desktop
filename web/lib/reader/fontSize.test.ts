import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_FONT_SIZE, loadFontSize, saveFontSize, stepFontSize } from './fontSize'

const STORAGE_KEY = 'livtet.reader.fontSize'

function memoryStorage(): Storage {
  const map = new Map<string, string>()
  return {
    get length() {
      return map.size
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => {
      map.delete(key)
    },
    setItem: (key: string, value: string) => {
      map.set(key, value)
    },
  }
}

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

describe('persistence', () => {
  beforeEach(() => vi.stubGlobal('localStorage', memoryStorage()))
  afterEach(() => vi.unstubAllGlobals())

  it('returns the default when nothing is stored', () => {
    expect(loadFontSize()).toBe(DEFAULT_FONT_SIZE)
  })

  it('restores the size the reader last chose', () => {
    saveFontSize(1.25)
    expect(loadFontSize()).toBe(1.25)
  })

  it('ignores stored values outside the range Readium accepts', () => {
    localStorage.setItem(STORAGE_KEY, '9')
    expect(loadFontSize()).toBe(DEFAULT_FONT_SIZE)

    localStorage.setItem(STORAGE_KEY, '')
    expect(loadFontSize()).toBe(DEFAULT_FONT_SIZE)
  })

  it('falls back to the default when storage is unavailable', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('denied')
      },
    })
    expect(loadFontSize()).toBe(DEFAULT_FONT_SIZE)
  })

  it('stays quiet when storage refuses the write', () => {
    vi.stubGlobal('localStorage', {
      setItem: () => {
        throw new Error('quota exceeded')
      },
    })
    expect(() => saveFontSize(1.5)).not.toThrow()
  })
})
