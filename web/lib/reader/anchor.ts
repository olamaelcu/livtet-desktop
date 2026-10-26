/** Block-level elements a reading-position anchor may land on. */
const ANCHORABLE = 'p, li, blockquote, dd, h1, h2, h3, h4, h5, h6, section, article'

/** Headings name the spot but make weak quotes; a paragraph is preferred. */
const HEADING = /^H[1-6]$/

/**
 * Longest block quote kept when no text is selected.
 *
 * Only bounds the whole-block fallback: a `section`/`article` ancestor could
 * otherwise store an entire chapter. A selection is never truncated — the
 * reader chose exactly that text, and the quote is what re-finds it.
 */
const MAX_BLOCK_QUOTE = 200

/** Characters of surrounding text kept so a repeated phrase resolves correctly. */
const MAX_CONTEXT = 48

/**
 * Probe points as fractions of the reading pane, top-to-bottom then left-to-right.
 *
 * The first hit inside a text node wins, which is the first line of text the
 * reader can see — the reading position in both paged and scrolled layouts. A
 * single fixed point cannot be used: in paginated mode the frame is the full
 * pane height and its top edge is the document's top, not the visible column.
 */
const PROBE_POINTS: ReadonlyArray<readonly [number, number]> = [
  [0.12, 0.08],
  [0.12, 0.2],
  [0.12, 0.35],
  [0.4, 0.08],
  [0.4, 0.2],
  [0.4, 0.35],
]

/**
 * A text anchor for a locator's `text` field.
 *
 * `before`/`after` are the context the quote resolver uses to tell apart
 * repeated phrases: without them it highlights the first occurrence in the
 * resource, which lands the note earlier than the text the reader picked.
 */
export interface Quote {
  highlight: string
  before?: string
  after?: string
}

/** Collapse runs of whitespace so a quote matches the rendered text. */
function collapse(text: string | null | undefined): string {
  return (text ?? '').replace(/\s+/g, ' ').trim()
}

/** The frame the reader is showing, ignoring pooled frames kept hidden. */
function visibleFrame(container: HTMLElement): HTMLIFrameElement | undefined {
  const frames = [...container.querySelectorAll('iframe')].filter((frame) => {
    const box = frame.getBoundingClientRect()
    if (box.width < 50 || box.height < 50) return false
    const styles = frame.ownerDocument.defaultView?.getComputedStyle(frame)
    if (!styles) return false
    return (
      styles.visibility === 'visible' &&
      styles.display !== 'none' &&
      Number.parseFloat(styles.opacity || '1') > 0
    )
  })
  // The pool paints later frames on top of earlier ones.
  return frames[frames.length - 1]
}

/** Text node under a pane-relative point, via the platform caret hit-test. */
function textNodeAt(doc: Document, x: number, y: number): Text | undefined {
  const range = doc.caretRangeFromPoint?.(x, y)
  const node = range?.startContainer
  if (node?.nodeType === Node.TEXT_NODE) return node as Text
  const position = doc.caretPositionFromPoint?.(x, y)
  const fallback = position?.offsetNode
  return fallback?.nodeType === Node.TEXT_NODE ? (fallback as Text) : undefined
}

/** Text before/after `range`, clipped to the surrounding block. */
function contextAround(range: Range): { before?: string; after?: string } {
  const block =
    range.commonAncestorContainer.nodeType === Node.ELEMENT_NODE
      ? (range.commonAncestorContainer as Element).closest(ANCHORABLE)
      : range.commonAncestorContainer.parentElement?.closest(ANCHORABLE)

  try {
    if (block) {
      const head = block.ownerDocument.createRange()
      head.setStart(block, 0)
      head.setEnd(range.startContainer, range.startOffset)
      const tail = block.ownerDocument.createRange()
      tail.setStart(range.endContainer, range.endOffset)
      tail.setEnd(block, block.childNodes.length)
      const before = collapse(head.toString()).slice(-MAX_CONTEXT)
      const after = collapse(tail.toString()).slice(0, MAX_CONTEXT)
      return { before: before || undefined, after: after || undefined }
    }
    // No containing block (text directly in the body): use the whole document.
    const doc = range.startContainer.ownerDocument ?? document
    const head = doc.createRange()
    head.setStart(doc.body, 0)
    head.setEnd(range.startContainer, range.startOffset)
    const tail = doc.createRange()
    tail.setStart(range.endContainer, range.endOffset)
    tail.setEnd(doc.body, doc.body.childNodes.length)
    return {
      before: collapse(head.toString()).slice(-MAX_CONTEXT) || undefined,
      after: collapse(tail.toString()).slice(0, MAX_CONTEXT) || undefined,
    }
  } catch {
    return {}
  }
}

/**
 * The reader's current selection in the publication, with context.
 *
 * Read straight from the frame's DOM rather than from the navigator's
 * `textSelected` event: the event only carries the selected string, and the
 * surrounding context is what keeps a repeated phrase from resolving to an
 * earlier occurrence.
 */
export function selectedQuote(container: HTMLElement): Quote | undefined {
  const frame = visibleFrame(container)
  const doc = frame?.contentDocument
  const selection = frame?.contentWindow?.getSelection()
  if (!doc || !selection || selection.isCollapsed || selection.rangeCount === 0) return undefined

  const highlight = collapse(selection.toString())
  if (!highlight) return undefined

  const range = selection.getRangeAt(0)
  return { highlight, ...contextAround(range) }
}

/**
 * Anchor for the block at the reading position, with context, used when the
 * reader writes a note without selecting anything.
 *
 * A quote — not a CSS selector — because the navigator resolves a decoration
 * through `text.highlight` with the quote anchor, while a selector would have
 * to survive as a `Map` inside `locations.otherLocations`, which the message
 * layer does not carry.
 *
 * Returns `undefined` when no text can be probed.
 */
export function readingPositionQuote(container: HTMLElement): Quote | undefined {
  const frame = visibleFrame(container)
  const doc = frame?.contentDocument
  if (!frame || !doc) return undefined

  const pane = container.getBoundingClientRect()
  const paneLeft = pane.left + container.clientLeft
  const paneTop = pane.top + container.clientTop
  const width = container.clientWidth
  const height = container.clientHeight
  if (width < 50 || height < 50) return undefined

  const frameBox = frame.getBoundingClientRect()
  let heading: Quote | undefined
  for (const [fx, fy] of PROBE_POINTS) {
    const innerX = paneLeft + width * fx - frameBox.left
    const innerY = paneTop + height * fy - frameBox.top
    const node = textNodeAt(doc, innerX, innerY)
    if (!node) continue

    const block = node.parentElement?.closest(ANCHORABLE)
    // Inside a real block, quote the block; directly in the body, quote the line.
    const text = collapse(block ? block.textContent : node.textContent)
    if (!text) continue

    const quote: Quote = { highlight: text.slice(0, MAX_BLOCK_QUOTE) }
    // A heading alone ("Chapter 1") repeats in contents lists; keep looking for
    // the prose it introduces and fall back to the heading only if there is none.
    if (block && HEADING.test(block.tagName)) {
      heading ??= quote
      continue
    }
    return quote
  }
  return heading
}
