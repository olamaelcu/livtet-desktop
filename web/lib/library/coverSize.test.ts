import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  COVER_MIN_WIDTH,
  COVER_SIZES,
  coverSizeFromIndex,
  coverSizeIndex,
  coverSizeLabel,
  DEFAULT_COVER_SIZE,
  loadCoverSize,
  saveCoverSize,
} from './coverSize'

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

describe('COVER_MIN_WIDTH', () => {
  it('maps each size to its grid min column width', () => {
    expect(COVER_MIN_WIDTH).toEqual({
      tiny: '5rem',
      small: '6.5rem',
      medium: '8rem',
      large: '11rem',
      huge: '16rem',
    })
  })

  it('orders tiny below medium below huge', () => {
    expect(parseFloat(COVER_MIN_WIDTH.tiny)).toBeLessThan(parseFloat(COVER_MIN_WIDTH.medium))
    expect(parseFloat(COVER_MIN_WIDTH.medium)).toBeLessThan(parseFloat(COVER_MIN_WIDTH.huge))
  })
})

describe('coverSizeIndex', () => {
  it('round-trips every size through its index', () => {
    for (const size of COVER_SIZES) {
      expect(coverSizeFromIndex(coverSizeIndex(size))).toBe(size)
    }
  })

  it('falls back to the default for out-of-range indexes', () => {
    expect(coverSizeFromIndex(-1)).toBe(DEFAULT_COVER_SIZE)
    expect(coverSizeFromIndex(COVER_SIZES.length)).toBe(DEFAULT_COVER_SIZE)
  })
})

describe('coverSizeLabel', () => {
  it('capitalizes the size name', () => {
    expect(coverSizeLabel('tiny')).toBe('Tiny')
    expect(coverSizeLabel('medium')).toBe('Medium')
    expect(coverSizeLabel('huge')).toBe('Huge')
  })
})

describe('persistence', () => {
  beforeEach(() => vi.stubGlobal('localStorage', memoryStorage()))
  afterEach(() => vi.unstubAllGlobals())

  it('returns the default when nothing is stored', () => {
    expect(loadCoverSize()).toBe(DEFAULT_COVER_SIZE)
  })

  it('persists and reloads a chosen size', () => {
    saveCoverSize('huge')
    expect(loadCoverSize()).toBe('huge')
  })

  it('ignores stored values that are not known sizes', () => {
    localStorage.setItem('livtet.library.coverSize', 'gigantic')
    expect(loadCoverSize()).toBe(DEFAULT_COVER_SIZE)
  })

  it('falls back to the default when storage is unavailable', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('denied')
      },
    })
    expect(loadCoverSize()).toBe(DEFAULT_COVER_SIZE)
  })
})
