import type { Decoration } from '@readium/navigator'
import { Locator, LocatorLocations } from '@readium/shared'
import { DEFAULT_HIGHLIGHT, HIGHLIGHTS } from './highlights'
import type { Annotation } from './progress'

/** Decoration group owning every reader annotation highlight. */
export const ANNOTATION_GROUP = 'reader-annotations'

/**
 * Styling for the highlight drawn over annotated text; the tint is per note.
 *
 * `enforceContrast` defaults to true, which darkens the tint until it meets a
 * contrast ratio — muting a deliberately chosen colour. Keep it off so the
 * palette lands exactly as authored.
 */
export const ANNOTATION_STYLE = {
  type: 'highlight' as const,
  enforceContrast: false,
}

/** Decoration group owning the semantic-element styling, kept out of the note layer. */
export const SEMANTIC_GROUP = 'reader-semantic'

/**
 * One kind of semantic block, drawn with a single tint.
 *
 * Publications express the same ideas two ways: `epub:type` from the EPUB
 * Structural Semantics Vocabulary, and DPUB-ARIA `role` values (EPUB 3.3 puts
 * roles on the accessibility path, and some pipelines emit only those). A
 * family gathers both spellings of one idea, plus the vocabulary's deprecated
 * synonyms — `note` became `footnote`, `warning` became `notice`, `sidebar`
 * became the `aside` element — because published files still carry the old
 * names.
 *
 * Grouped by family rather than token so 25 tokens do not need 25 colours:
 * what the reader needs to recognise is the *kind* of block, not which
 * synonym the publisher's tool chose.
 */
export interface SemanticFamily {
  /** Stable key; carried in every decoration id of the family. */
  name: string
  /** Tint drawn under the family's elements, from the note palette. */
  tint: string
  /** `epub:type` tokens in this family. */
  epubTypes: string[]
  /** DPUB-ARIA roles in this family, for resources that carry `role` alone. */
  roles?: string[]
}

/**
 * Families the reader tints, all of them blocks that sit *inside* running text.
 *
 * Document-level semantics (`part`, `chapter`, `frontmatter`, `preface`,
 * `titlepage`, `colophon`, …) are deliberately absent: their elements span
 * whole pages or chapters, so a wash over them paints the page rather than
 * marking a passage. Same for `table`/`list`/`figure`, which are too pervasive
 * to colour.
 */
export const SEMANTIC_FAMILIES: SemanticFamily[] = [
  {
    name: 'warning',
    tint: HIGHLIGHTS.yellow.tint,
    epubTypes: ['warning', 'notice'],
    roles: ['doc-notice'],
  },
  {
    // Not in the vocabulary: DocBook and AsciiDoc pipelines emit it, and it
    // reads to a reader as its own severity rather than as a warning.
    name: 'caution',
    tint: HIGHLIGHTS.pink.tint,
    epubTypes: ['caution'],
  },
  {
    name: 'tip',
    tint: HIGHLIGHTS.green.tint,
    epubTypes: ['tip', 'help'],
    roles: ['doc-tip'],
  },
  {
    name: 'note',
    tint: HIGHLIGHTS.blue.tint,
    epubTypes: ['note', 'footnote', 'footnotes', 'endnote', 'endnotes', 'rearnote', 'rearnotes'],
    roles: ['doc-footnote', 'doc-endnote', 'doc-endnotes'],
  },
  {
    name: 'aside',
    tint: HIGHLIGHTS.purple.tint,
    epubTypes: ['aside', 'sidebar', 'marginalia', 'annotation', 'epigraph', 'pullquote'],
    roles: ['doc-epigraph', 'doc-pullquote'],
  },
]

/**
 * CSS selector matching every element of one family.
 *
 * `epub:type` lives in the IDPF ops namespace in XHTML, and Readium serialises
 * each resource into a blob typed as the resource's own media type, which the
 * parser treats as XML. In XML an attribute selector with no namespace matches
 * only un-namespaced attributes, so `[epub\:type~="…"]` — and the qualified
 * `[*|epub\:type~="…"]` — find nothing. `[*|type~="…"]` matches the local name
 * in any namespace, which covers both the namespaced XHTML attribute and the
 * un-namespaced one an HTML-parsed resource produces. `role` is un-namespaced
 * in both parsings, so it needs no such pairing.
 */
function familySelector(family: SemanticFamily): string {
  return [
    ...family.epubTypes.flatMap((token) => [`[epub\\:type~="${token}"]`, `[*|type~="${token}"]`]),
    ...(family.roles ?? []).map((role) => `[role~="${role}"]`),
  ].join(', ')
}

/**
 * A locator that carries only a CSS selector, for a decoration spanning an
 * element rather than a text range.
 *
 * RWPM flattens a locator's extra locations into `locations` itself, and the
 * receiving injectable rebuilds its map from those unknown keys — so the
 * selector is written as an own property, not only as the `otherLocations`
 * entry. The installed `@readium/shared` build lacks the branch that also
 * accepts a live `otherLocations` map over the wire, and a decoration whose
 * locator arrives without a selector is dropped for want of a DOM range.
 */
function semanticLocator(href: string, cssSelector: string): Locator {
  const locations = new LocatorLocations({
    otherLocations: new Map([['cssSelector', cssSelector]]),
  })
  ;(locations as unknown as Record<string, unknown>).cssSelector = cssSelector
  return new Locator({ href, type: 'text/html', locations })
}

/**
 * Style the publication's semantic blocks.
 *
 * A decoration is sent to a frame only when its locator's `href` equals that
 * frame's resource, so one decoration is built per (family, resource) pair. The
 * locator carries no text or DOM range: the injectable resolves the CSS
 * selector to the element and ranges over it, which reflows with the text.
 * Ids embed the resource because decorations are diffed by id.
 */
export function buildSemanticDecorations(
  families: SemanticFamily[],
  hrefs: string[],
): Decoration[] {
  return families.flatMap((family) => {
    const cssSelector = familySelector(family)
    return hrefs.map((href) => ({
      id: `${SEMANTIC_GROUP}:${family.name}:${href}`,
      locator: semanticLocator(href, cssSelector),
      style: { ...ANNOTATION_STYLE, tint: family.tint },
    }))
  })
}

/**
 * Selector for page-break markers, in every spelling a publication may use.
 *
 * A break is an empty `<span epub:type="pagebreak"/>`, so it carries no text to
 * tint and several of them share one resource — a decoration cannot express it:
 * its selector resolves to a single element, so a chapter with forty breaks
 * would get one mark. The rules below are injected into the frame instead and
 * paint every break in the document.
 */
const PAGE_BREAK_SELECTOR =
  '[epub\\:type~="pagebreak"], [*|type~="pagebreak"], [role~="doc-pagebreak"]'

/**
 * Styles the reader writes into each publication frame.
 *
 * The empty break span gets the tick from a pseudo-element rather than from its
 * own box: Readium's reading styles force `background-color: transparent
 * !important` on every element (so a theme's background can show through), and
 * a `*` reset does not reach pseudo-elements. Painting the span itself would
 * also make an inline marker into a box, nudging the text around it.
 * `currentColor` keeps the tick on the reader's theme.
 */
export const SEMANTIC_STYLESHEET = `${PAGE_BREAK_SELECTOR.split(', ')
  .map((selector) => `${selector}::after`)
  .join(',\n')} {
  content: "";
  display: inline-block;
  width: 2px;
  height: 0.9em;
  margin-inline: 0.15em;
  border-radius: 1px;
  background: currentColor;
  opacity: 0.3;
  vertical-align: middle;
}
`

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
    const tint = HIGHLIGHTS[annotation.color ?? DEFAULT_HIGHLIGHT]?.tint ?? HIGHLIGHTS.yellow.tint
    return [{ id: annotation.id, locator, style: { ...ANNOTATION_STYLE, tint } }]
  })
}
