import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  COVER_SCALE,
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

describe('COVER_SCALE', () => {
  it('maps each fixed size to its multiplier', () => {
    expect(COVER_SCALE).toEqual({
      tiny: 0.5,
      small: 0.7,
      medium: 1,
      large: 2,
      huge: 3,
    })
  })

  it('orders tiny below medium below large', () => {
    expect(COVER_SCALE.tiny).toBeLessThan(COVER_SCALE.medium)
    expect(COVER_SCALE.medium).toBeLessThan(COVER_SCALE.large)
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
