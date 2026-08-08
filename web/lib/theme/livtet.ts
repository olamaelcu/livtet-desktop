import tokens from '@olamaelcu/livtet-branding/tokens.json'
import type { OklchTriplet } from './types'

interface OklchToken {
  chroma: number
  hue: number
  lightness: number
}

function toTriplet(t: OklchToken): OklchTriplet {
  return { l: t.lightness, c: t.chroma, h: t.hue }
}

export const livtetBrand = toTriplet(tokens.color.brand.oklch)
export const livtetNeutral = toTriplet(tokens.color.neutral.oklch)
