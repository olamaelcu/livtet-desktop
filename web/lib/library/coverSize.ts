export const COVER_SIZES = ['tiny', 'small', 'medium', 'large', 'huge'] as const

export type CoverSize = (typeof COVER_SIZES)[number]

export const COVER_MIN_WIDTH: Record<CoverSize, string> = {
  tiny: '4.5rem',
  small: '6rem',
  medium: '8rem',
  large: '11rem',
  huge: '14rem',
}

export const DEFAULT_COVER_SIZE: CoverSize = 'medium'

const STORAGE_KEY = 'livtet.library.coverSize'

export function coverSizeIndex(size: CoverSize): number {
  return COVER_SIZES.indexOf(size)
}

export function coverSizeFromIndex(index: number): CoverSize {
  return COVER_SIZES[index] ?? DEFAULT_COVER_SIZE
}

export function coverSizeLabel(size: CoverSize): string {
  return size.charAt(0).toUpperCase() + size.slice(1)
}

function isCoverSize(value: string): value is CoverSize {
  return (COVER_SIZES as readonly string[]).includes(value)
}

export function loadCoverSize(): CoverSize {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    return stored !== null && isCoverSize(stored) ? stored : DEFAULT_COVER_SIZE
  } catch {
    return DEFAULT_COVER_SIZE
  }
}

export function saveCoverSize(size: CoverSize): void {
  try {
    localStorage.setItem(STORAGE_KEY, size)
  } catch {
    return
  }
}
