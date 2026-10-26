import { expect, test } from 'vitest'
import { readingPositionQuote, selectedQuote } from '$lib/reader/anchor'

/**
 * Mount a container holding an iframe whose document has a paragraph, laid out
 * where `readingPositionQuote` probes so `elementFromPoint` lands inside it.
 */
async function mountPane(html: string): Promise<HTMLElement> {
  const pane = document.createElement('div')
  pane.style.cssText = 'position: fixed; inset: 0; width: 400px; height: 300px; display: flex'

  const frame = document.createElement('iframe')
  frame.style.cssText = 'border: 0; width: 400px; height: 300px'
  frame.srcdoc = `<body style="margin:0;font:16px serif">${html}</body>`

  const { promise: loaded, resolve } = Promise.withResolvers<void>()
  frame.addEventListener('load', () => resolve(), { once: true })
  pane.append(frame)
  document.body.append(pane)
  await loaded
  return pane
}

test('quotes the paragraph at the reading position', async () => {
  const pane = await mountPane('<p style="margin:0">The pattern master watched the city burn.</p>')

  expect(readingPositionQuote(pane)?.highlight).toBe('The pattern master watched the city burn.')
  pane.remove()
})

test('collapses whitespace and caps the whole-block fallback', async () => {
  const long = 'alpha '.repeat(80)
  const pane = await mountPane(`<p style="margin:0">${long}</p>`)

  const quote = readingPositionQuote(pane)?.highlight
  expect(quote?.startsWith('alpha alpha')).toBe(true)
  expect(quote?.length).toBeLessThanOrEqual(200)
  expect(quote).not.toContain('  ')
  pane.remove()
})

test('returns undefined when the pane is too small to probe', () => {
  const pane = document.createElement('div')
  pane.style.cssText = 'position: fixed; inset: 0; width: 10px; height: 10px'
  document.body.append(pane)

  expect(readingPositionQuote(pane)).toBeUndefined()
  pane.remove()
})

test('quotes a visible block, never the whole chapter body', async () => {
  // Paginated frames put a stylesheet and heading at the top of the document;
  // a naive probe quotes `<body>` and lands the highlight at the chapter start.
  const pane = await mountPane(
    '<style>html, body { margin: 0; column-width: 20em; }</style>' +
      '<h1 style="margin:0">Chapter 1: The Pattern Master</h1>' +
      '<p style="margin:0">But the state of neither her typewriter nor her bank account</p>' +
      '<p style="margin:0">was anything to boast about that year.</p>',
  )

  const quote = readingPositionQuote(pane)?.highlight
  expect(quote).toBeTruthy()
  expect(quote).not.toContain('{')
  expect(quote).not.toContain('column-width')
  expect([
    'Chapter 1: The Pattern Master',
    'But the state of neither her typewriter nor her bank account',
    'was anything to boast about that year.',
  ]).toContain(quote)
  pane.remove()
})

test('skips the heading above the prose when anchoring', async () => {
  const pane = await mountPane(
    '<h1 style="margin:0">Chapter 1</h1>' +
      '<p style="margin:0">The bus rumbled to a stop in front of a small group of people.</p>',
  )

  expect(readingPositionQuote(pane)?.highlight).toBe(
    'The bus rumbled to a stop in front of a small group of people.',
  )
  pane.remove()
})

test('falls back to the heading when the pane shows nothing else', async () => {
  const pane = await mountPane('<h1 style="margin:0;height:280px">Chapter 1</h1>')

  expect(readingPositionQuote(pane)?.highlight).toBe('Chapter 1')
  pane.remove()
})

/** Select a substring inside the pane's frame, as a drag would. */
function selectInFrame(pane: HTMLElement, paragraphIndex: number, phrase: string): void {
  const doc = (pane.querySelector('iframe') as HTMLIFrameElement).contentDocument
  const paragraph = doc?.querySelectorAll('p')[paragraphIndex]
  const node = paragraph?.firstChild
  const content = node?.textContent
  if (!doc || !node || !content) throw new Error('paragraph missing')
  const start = content.indexOf(phrase)
  const range = doc.createRange()
  range.setStart(node, start)
  range.setEnd(node, start + phrase.length)
  const selection = doc.getSelection()
  selection?.removeAllRanges()
  selection?.addRange(range)
}

test('quotes the live selection with surrounding context', async () => {
  const pane = await mountPane(
    '<p style="margin:0">Opening words. Chosen sentence. Closing words.</p>',
  )
  selectInFrame(pane, 0, 'Chosen sentence.')

  const quote = selectedQuote(pane)
  expect(quote?.highlight).toBe('Chosen sentence.')
  expect(quote?.before).toContain('Opening words.')
  expect(quote?.after).toContain('Closing words.')
  pane.remove()
})

test('context distinguishes a repeated sentence from its earlier twin', async () => {
  // Both paragraphs end with the same sentence; without context the decorator
  // would highlight the first one, placing the note too early in the chapter.
  const pane = await mountPane(
    '<p style="margin:0">Lead in. The same sentence here.</p>' +
      '<p style="margin:0">Later lead. The same sentence here.</p>',
  )
  selectInFrame(pane, 1, 'The same sentence here.')

  const quote = selectedQuote(pane)
  expect(quote?.before).toContain('Later lead.')
  pane.remove()
})

test('reports no selection when the reader has not picked any text', async () => {
  const pane = await mountPane('<p style="margin:0">Nothing selected.</p>')

  expect(selectedQuote(pane)).toBeUndefined()
  pane.remove()
})

test('keeps a long selection in full', async () => {
  const long = 'the reader chose every one of these words. '.repeat(12).trim()
  const pane = await mountPane(`<p style="margin:0">${long}</p>`)
  selectInFrame(pane, 0, long)

  const quote = selectedQuote(pane)
  expect(quote?.highlight).toBe(long)
  expect(quote?.highlight.length).toBeGreaterThan(200)
  pane.remove()
})
