import { fontSizeRangeConfig } from '@readium/navigator'

/** The text size Readium CSS applies when no preference overrides it: 100% zoom. */
export const DEFAULT_FONT_SIZE = 1

const [MIN_FONT_SIZE, MAX_FONT_SIZE] = fontSizeRangeConfig.range
const FONT_SIZE_STEP = fontSizeRangeConfig.step
/** Precision of the step, so stepping does not leave 0.9999999999999999 behind. */
const FONT_SIZE_DECIMALS = String(FONT_SIZE_STEP).split('.')[1]?.length ?? 0

/**
 * Apply one text-size step, clamped to the range Readium accepts.
 *
 * Text size is a multiplier on the frame's zoom, and Readium's own preference editor steps
 * it in fifths of a hundredth, so the arithmetic is rounded to the step's precision rather
 * than left to binary floating point.
 */
export function stepFontSize(current: number, direction: 1 | -1): number {
  const next = current + direction * FONT_SIZE_STEP
  const rounded = Math.round(next * 10 ** FONT_SIZE_DECIMALS) / 10 ** FONT_SIZE_DECIMALS
  return Math.min(Math.max(rounded, MIN_FONT_SIZE), MAX_FONT_SIZE)
}
