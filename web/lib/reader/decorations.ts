import type { Decoration } from '@readium/navigator'
import { Locator } from '@readium/shared'
import type { Annotation } from './progress'

/**
 * Styling for the highlight drawn over annotated text.
 *
 * `enforceContrast` defaults to true, which darkens the tint until it meets a
 * contrast ratio — muting a deliberate colour. Keep it off so the tint lands
 * exactly as authored. A 6-digit hex plus alpha keeps the text legible over
 * both light and dark reader themes.
 */
export const ANNOTATION_STYLE = {
  type: 'highlight' as const,
  tint: 'rgba(255, 213, 79, 0.55)',
  enforceContrast: false,
}

/**
 * The subset of a serialized Readium locator that decides whether a highlight
 * can be drawn.
 *
 * The navigator resolves a decoration to a DOM range from, in order: a saved
 * `locations.domRange`, a text fragment, the quoted text (`text.highlight`),
 * then a CSS selector or element `fragments`. A bare position locator — what
 * `positionChanged` emits — carries none of these, so it cannot be ranged.
 */
interface StoredLocator {
  text?: { highlight?: string }
  locations?: {
    domRange?: unknown
    fragments?: unknown[]
    cssSelector?: string
  }
}

function parseLocator(raw: string | undefined): StoredLocator | undefined {
  if (!raw) return undefined
  try {
    const parsed: unknown = JSON.parse(raw)
    return parsed !== null && typeof parsed === 'object' ? (parsed as StoredLocator) : undefined
  } catch {
    return undefined
  }
}

/**
 * Whether an annotation's locator can be resolved to text, and therefore
 * shown as a highlight over the publication.
 */
export function isAnchorable(annotation: Annotation): boolean {
  const locator = parseLocator(annotation.locator)
  if (!locator) return false
  if (typeof locator.text?.highlight === 'string' && locator.text.highlight.trim() !== '') {
    return true
  }
  const locations = locator.locations
  if (!locations) return false
  if (locations.domRange) return true
  if (Array.isArray(locations.fragments) && locations.fragments.length > 0) return true
  return typeof locations.cssSelector === 'string' && locations.cssSelector !== ''
}

/**
 * Turn stored annotations into navigator decorations.
 *
 * Each decoration is anchored at the annotation's saved locator, so the
 * highlight lands on the annotated text and reflows with it. Annotations whose
 * locator cannot be resolved to text are skipped: the navigator would only log
 * "Can't locate DOM range" for them. Malformed JSON is tolerated so one bad row
 * cannot blank the whole layer.
 */
export function buildAnnotationDecorations(annotations: Annotation[]): Decoration[] {
  return annotations.flatMap((annotation) => {
    if (!isAnchorable(annotation) || !annotation.locator) return []
    let locator: Locator | undefined
    try {
      locator = Locator.deserialize(JSON.parse(annotation.locator))
    } catch {
      return []
    }
    if (!locator) return []
    return [{ id: annotation.id, locator, style: ANNOTATION_STYLE }]
  })
}
