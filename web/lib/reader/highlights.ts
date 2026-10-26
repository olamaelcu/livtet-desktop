/**
 * Highlight colours a note can carry.
 *
 * A note stores the *key*, not the CSS value, so the palette can be retuned
 * (or made theme-aware) without rewriting saved notes. Tints are translucent
 * so the text underneath stays legible on light and dark reader themes.
 */
export const HIGHLIGHTS = {
  yellow: { label: 'Yellow', tint: 'rgba(255, 213, 79, 0.55)' },
  green: { label: 'Green', tint: 'rgba(129, 199, 132, 0.55)' },
  blue: { label: 'Blue', tint: 'rgba(100, 181, 246, 0.5)' },
  pink: { label: 'Pink', tint: 'rgba(244, 143, 177, 0.5)' },
  purple: { label: 'Purple', tint: 'rgba(179, 157, 219, 0.55)' },
} as const satisfies Record<string, { label: string; tint: string }>

/** One of the palette keys above. */
export type HighlightColor = keyof typeof HIGHLIGHTS

/** Colour used when a note carries none, and for unknown stored keys. */
export const DEFAULT_HIGHLIGHT: HighlightColor = 'yellow'

/** Palette keys in picker order. */
export const HIGHLIGHT_COLORS = Object.keys(HIGHLIGHTS) as HighlightColor[]
