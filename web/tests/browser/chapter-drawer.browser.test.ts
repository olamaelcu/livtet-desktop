// The drawer's rows are the publication's reading order, and choosing one has
// to hand back the chapter it belongs to — a `wa-drawer` slots its content, so
// only a real element shows the rows and the current-chapter marking.
import '@awesome.me/webawesome/dist/components/drawer/drawer.js'
import { afterEach, expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import ChapterDrawer from '$lib/components/ChapterDrawer.svelte'
import type { Chapter } from '$lib/reader/chapters'

const XHTML = 'application/xhtml+xml'

const chapters: Chapter[] = [
  { href: 'OEBPS/cover.xhtml', type: XHTML, title: 'Cover' },
  { href: 'OEBPS/ch01.xhtml', type: XHTML, title: 'The Long Way' },
]

function rows(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>('.chapter')]
}

afterEach(() => {
  for (const element of document.querySelectorAll('wa-drawer')) element.remove()
})

test('lists every chapter of the reading order by title', async () => {
  const { container } = await render(ChapterDrawer, {
    props: { chapters, currentHref: '', onselect: () => {}, onclose: () => {} },
  })
  // The rows are slotted into the drawer, so they only appear once the element
  // has upgraded and rendered.
  await customElements.whenDefined('wa-drawer')

  expect(rows(container).map((row) => row.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
    'Cover',
    'The Long Way',
  ])
})

test('marks the chapter the publication is on', async () => {
  const { container } = await render(ChapterDrawer, {
    props: { chapters, currentHref: 'OEBPS/ch01.xhtml', onselect: () => {}, onclose: () => {} },
  })

  const [cover, current] = rows(container)
  expect(cover?.getAttribute('aria-current')).toBeNull()
  expect(current?.getAttribute('aria-current')).toBe('true')
})

test('reports the chapter a reader picks', async () => {
  const picked: Chapter[] = []
  const { container } = await render(ChapterDrawer, {
    props: {
      chapters,
      currentHref: '',
      onselect: (chapter) => picked.push(chapter),
      onclose: () => {},
    },
  })

  rows(container)[1]?.click()

  expect(picked).toEqual([chapters[1]])
})

test('says so when the publication names no chapters', async () => {
  const { container } = await render(ChapterDrawer, {
    props: { chapters: [], currentHref: '', onselect: () => {}, onclose: () => {} },
  })

  expect(rows(container)).toHaveLength(0)
  expect(container.textContent).toContain('names no chapters')
})
