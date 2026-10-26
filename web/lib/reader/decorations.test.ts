import { describe, expect, it } from 'vitest'
import { ANNOTATION_STYLE, buildAnnotationDecorations, isAnchorable } from './decorations'
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
