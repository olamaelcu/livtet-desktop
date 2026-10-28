import { describe, expect, it } from 'vitest'
import {
  ANNOTATION_STYLE,
  buildAnnotationDecorations,
  buildSemanticDecorations,
  isAnchorable,
  SEMANTIC_FAMILIES,
  SEMANTIC_GROUP,
  SEMANTIC_STYLESHEET,
} from './decorations'
import { HIGHLIGHTS } from './highlights'

const selectionLocator = (highlight: string) =>
  JSON.stringify({
    href: 'ch01.xhtml',
    type: 'application/xhtml+xml',
    text: { highlight },
  })

const positionLocator = () =>
  JSON.stringify({
    href: 'ch01.xhtml',
    type: 'application/xhtml+xml',
    locations: { progression: 0.25, totalProgression: 0.1, position: 3 },
  })

describe('anchorable annotations', () => {
  it('accepts a locator quoting the selected text', () => {
    expect(
      isAnchorable({ id: 'a1', text: 'note', locator: selectionLocator('the sentence') }),
    ).toBe(true)
  })

  it('rejects a bare position locator, which cannot be resolved to text', () => {
    expect(isAnchorable({ id: 'a1', text: 'note', locator: positionLocator() })).toBe(false)
  })

  it('accepts locators carrying fragments, a css selector, or a dom range', () => {
    const withFragments = JSON.stringify({
      href: 'ch01.xhtml',
      type: 'application/xhtml+xml',
      locations: { fragments: ['p-3'] },
    })
    const withSelector = JSON.stringify({
      href: 'ch01.xhtml',
      type: 'application/xhtml+xml',
      locations: { cssSelector: 'p:nth-child(3)' },
    })
    const withRange = JSON.stringify({
      href: 'ch01.xhtml',
      type: 'application/xhtml+xml',
      locations: { domRange: { start: {}, end: {} } },
    })

    expect(isAnchorable({ id: 'f', text: 'n', locator: withFragments })).toBe(true)
    expect(isAnchorable({ id: 's', text: 'n', locator: withSelector })).toBe(true)
    expect(isAnchorable({ id: 'r', text: 'n', locator: withRange })).toBe(true)
  })

  it('rejects annotations without a locator', () => {
    expect(isAnchorable({ id: 'none', text: 'note' })).toBe(false)
  })
})

describe('annotation decorations', () => {
  it('anchors one highlight per anchorable annotation', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'a1', text: 'one', locator: selectionLocator('first sentence') },
      { id: 'a2', text: 'two', locator: selectionLocator('second sentence') },
    ])

    expect(decorations.map((d) => d.id)).toEqual(['a1', 'a2'])
    expect(decorations[0]?.locator.href).toBe('ch01.xhtml')
    expect(decorations[0]?.style).toEqual({
      ...ANNOTATION_STYLE,
      tint: HIGHLIGHTS.yellow.tint,
    })
  })

  it('skips position-only annotations so the navigator never warns', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'position-only', text: 'written without a selection', locator: positionLocator() },
      { id: 'a1', text: 'one', locator: selectionLocator('first sentence') },
    ])

    expect(decorations.map((d) => d.id)).toEqual(['a1'])
  })

  it('tolerates malformed locator JSON instead of failing the whole layer', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'broken', text: 'bad row', locator: 'not json' },
      { id: 'a1', text: 'one', locator: selectionLocator('first sentence') },
    ])

    expect(decorations.map((d) => d.id)).toEqual(['a1'])
  })

  it('skips locators the shared model rejects', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'empty', text: 'empty object', locator: '{"text":{"highlight":"x"}}' },
      { id: 'null', text: 'null', locator: 'null' },
    ])

    expect(decorations).toEqual([])
  })
})

describe('highlight colours', () => {
  it('tints each note with its own colour', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'y', text: 'one', locator: selectionLocator('first'), color: 'yellow' },
      { id: 'g', text: 'two', locator: selectionLocator('second'), color: 'green' },
      { id: 'b', text: 'three', locator: selectionLocator('third'), color: 'blue' },
    ])

    expect(decorations.map((d) => d.style)).toEqual([
      { ...ANNOTATION_STYLE, tint: HIGHLIGHTS.yellow.tint },
      { ...ANNOTATION_STYLE, tint: HIGHLIGHTS.green.tint },
      { ...ANNOTATION_STYLE, tint: HIGHLIGHTS.blue.tint },
    ])
  })

  it('falls back to the default colour for notes without one', () => {
    const decorations = buildAnnotationDecorations([
      { id: 'none', text: 'no colour', locator: selectionLocator('text') },
    ])

    expect(decorations[0]?.style).toEqual({ ...ANNOTATION_STYLE, tint: HIGHLIGHTS.yellow.tint })
  })

  it('ignores a colour key that is no longer in the palette', () => {
    const decorations = buildAnnotationDecorations([
      {
        id: 'stale',
        text: 'old palette',
        locator: selectionLocator('text'),
        // A note written by an older/newer palette must still render.
        color: 'chartreuse' as never,
      },
    ])

    expect(decorations[0]?.style).toEqual({ ...ANNOTATION_STYLE, tint: HIGHLIGHTS.yellow.tint })
  })
})

describe('semantic decorations', () => {
  const hrefs = ['ch01.xhtml', 'ch02.xhtml']
  function family(name: string) {
    const found = SEMANTIC_FAMILIES.find((item) => item.name === name)
    if (!found) throw new Error(`unknown family: ${name}`)
    return found
  }

  it('emits one decoration per family and resource', () => {
    const decorations = buildSemanticDecorations(SEMANTIC_FAMILIES, hrefs)

    expect(decorations).toHaveLength(SEMANTIC_FAMILIES.length * hrefs.length)
    expect(decorations.map((d) => d.locator.href)).toEqual(SEMANTIC_FAMILIES.flatMap(() => hrefs))
    expect(new Set(decorations.map((d) => d.id)).size).toBe(decorations.length)
    expect(decorations[0]?.id).toContain(SEMANTIC_GROUP)
  })

  it('carries the selector as a plain location key for the frame', () => {
    const [decoration] = buildSemanticDecorations([family('warning')], ['ch01.xhtml'])

    // The decoration reaches the frame as a structured clone of a plain object.
    // The frame rebuilds its location map from the unknown keys of `locations`,
    // and the shipped `@readium/shared` build does not read an in-memory map, so
    // a selector held only there arrives empty and the decoration is dropped for
    // want of a DOM range.
    const wire = structuredClone(decoration?.locator) as unknown as {
      locations: Record<string, unknown>
    }

    expect(wire.locations.cssSelector).toContain('[*|type~="warning"]')
  })

  it('matches both spellings of a family and its synonyms', () => {
    const selector = (name: string) =>
      buildSemanticDecorations([family(name)], ['ch01.xhtml'])[0]?.locator.serialize().locations
        .cssSelector as string

    // `epub:type` is namespaced in XHTML, where a bare attribute selector finds
    // nothing; `*|` also covers the un-namespaced attribute of an HTML resource.
    expect(selector('warning')).toContain('[epub\\:type~="notice"]')
    expect(selector('warning')).toContain('[*|type~="warning"]')
    // DPUB-ARIA, for publications that express the block with a role only.
    expect(selector('warning')).toContain('[role~="doc-notice"]')
    // Deprecated names stay matched: published files still carry them.
    expect(selector('aside')).toContain('[*|type~="sidebar"]')
    expect(selector('note')).toContain('[*|type~="footnote"]')
  })

  it('never puts one token in two families, so a block is tinted once', () => {
    const seen = new Set<string>()

    for (const item of SEMANTIC_FAMILIES) {
      for (const token of [...item.epubTypes, ...(item.roles ?? [])]) {
        expect(seen.has(token), `${token} appears in more than one family`).toBe(false)
        seen.add(token)
      }
    }
  })

  it('tints each family with the palette', () => {
    const decorations = buildSemanticDecorations(SEMANTIC_FAMILIES, ['ch01.xhtml'])

    expect(decorations.map((d) => d.style)).toEqual(
      SEMANTIC_FAMILIES.map((item) => ({ ...ANNOTATION_STYLE, tint: item.tint })),
    )
  })

  it('styles nothing without a resource to place it on', () => {
    expect(buildSemanticDecorations(SEMANTIC_FAMILIES, [])).toEqual([])
  })
})

describe('frame stylesheet', () => {
  it('turns page breaks into margin ticks in every spelling', () => {
    // A break is an empty span and a resource holds many of them, so a
    // decoration (one selector, one element) cannot mark them: the stylesheet
    // that the frame installs paints all of them.
    expect(SEMANTIC_STYLESHEET).toContain('[epub\\:type~="pagebreak"]')
    expect(SEMANTIC_STYLESHEET).toContain('[*|type~="pagebreak"]')
    expect(SEMANTIC_STYLESHEET).toContain('[role~="doc-pagebreak"]')
    expect(SEMANTIC_STYLESHEET).toContain('display: inline-block')
    expect(SEMANTIC_STYLESHEET).toContain('background: currentColor')
  })
})
