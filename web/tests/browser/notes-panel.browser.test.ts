import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { Annotation } from '$lib/reader/progress'
import NotesPanelFixture from './fixtures/NotesPanelFixture.svelte'

function note(overrides: Partial<Annotation> & { id: string }): Annotation {
  return { text: 'note text', ...overrides }
}

const dated = note({
  id: 'a',
  text: 'dated note',
  quote: 'the quoted sentence',
  color: 'green',
  createdAt: '2026-10-26T05:57:38.074Z',
})
const undated = note({ id: 'b', text: 'second note' })

function rows(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll('.note-row')] as HTMLElement[]
}

test('lists every note with its text', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  expect(rows(container)).toHaveLength(2)
  expect(container.querySelector('.panel-heading')?.textContent).toContain('Notes (2)')
  expect(container.textContent).toContain('dated note')
  expect(container.textContent).toContain('second note')
})

test('shows the creation date only for notes that have one', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  const times = [...container.querySelectorAll('time')]
  expect(times).toHaveLength(1)
  expect(times[0]?.getAttribute('datetime')).toBe('2026-10-26T05:57:38.074Z')
  expect(times[0]?.textContent).toContain('2026')
})

test('marks the note colour and reports a change', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  const [firstRow] = rows(container)
  const swatches = [...(firstRow?.querySelectorAll('.swatch') ?? [])]
  expect(swatches.map((s) => s.getAttribute('aria-pressed'))).toEqual([
    'false',
    'true',
    'false',
    'false',
    'false',
  ])

  ;(swatches[2] as HTMLElement).click()
  await expect.poll(() => events).toEqual(['color:dated note:blue'])
})

test('reports visit, edit and delete for the note they belong to', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  const secondRow = rows(container)[1]
  const buttons = [...(secondRow?.querySelectorAll('wa-button') ?? [])] as HTMLElement[]
  const byLabel = (label: string) =>
    buttons.find((button) => button.textContent?.includes(label)) as HTMLElement

  byLabel('Visit').click()
  byLabel('Edit').click()
  byLabel('Delete').click()

  await expect
    .poll(() => events)
    .toEqual(['visit:second note', 'edit:second note', 'delete:second note'])
})

type WaSearch = HTMLElement & { value: string; updateComplete?: Promise<unknown> }

function search(container: HTMLElement): WaSearch {
  const element = container.querySelector('wa-input.search')
  if (!element) throw new Error('panel has no search field')
  return element as WaSearch
}

/**
 * Type into the search field the way a reader does: `wa-input` keeps its real
 * `<input>` in shadow DOM, so set that value and fire a composed input event,
 * which is what reaches the host listener.
 */
async function typeSearch(container: HTMLElement, value: string): Promise<void> {
  const field = search(container)
  await field.updateComplete
  const inner = field.shadowRoot?.querySelector('input')
  if (!inner) throw new Error('search field has no input')
  inner.value = value
  inner.dispatchEvent(new Event('input', { bubbles: true, composed: true }))
}

test('filters the rows as the reader types', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  await typeSearch(container, 'dated')

  await expect.poll(() => rows(container)).toHaveLength(1)
  expect(container.textContent).toContain('dated note')
  expect(container.textContent).not.toContain('second note')
  // Heading reports the filtered count against the total.
  expect(container.querySelector('.panel-heading')?.textContent).toContain('(1/2)')
})

test('matches the quoted passage too', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  await typeSearch(container, 'quoted sentence')

  await expect.poll(() => rows(container)).toHaveLength(1)
  expect(container.textContent).toContain('dated note')
})

test('says so when nothing matches, and clears back to every note', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [dated, undated], events },
  })

  await typeSearch(container, 'zebra')

  await expect.poll(() => rows(container)).toHaveLength(0)
  expect(container.querySelector('.empty')?.textContent).toContain('zebra')

  const clear = [...container.querySelectorAll('.search wa-button')][0] as HTMLElement
  clear.click()

  await expect.poll(() => rows(container)).toHaveLength(2)
  expect(container.querySelector('.empty')).toBeNull()
})

test('lists bookmarks apart from notes, without note text', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: {
      notes: [dated],
      bookmarks: [
        { id: 'b1', kind: 'pdf', pageNumber: 12, createdAt: '2026-10-26T05:57:38.074Z' },
        { id: 'b2', kind: 'timestamp', positionSeconds: 2527 },
      ],
      events,
    },
  })

  expect(container.querySelectorAll('.bookmark-row')).toHaveLength(2)
  expect(container.textContent).toContain('Page 12')
  expect(container.textContent).toContain('42:07')
  expect(container.querySelector('.section-heading')?.textContent).toContain('Bookmarks (2)')
  // Notes keep their own list and remain the only source of quote text.
  expect(rows(container)).toHaveLength(1)
})

test('reports bookmark visits and removals', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: {
      notes: [],
      bookmarks: [{ id: 'b1', kind: 'pdf', pageNumber: 12 }],
      events,
    },
  })

  const [row] = [...container.querySelectorAll('.bookmark-row')]
  const [visit, remove] = [...(row?.querySelectorAll('wa-button') ?? [])] as HTMLElement[]

  visit?.click()
  remove?.click()

  expect(events).toEqual(['visitbookmark:b1', 'removebookmark:b1'])
})

test('does not claim there are no notes when the edition only has bookmarks', async () => {
  const events: string[] = []
  const { container } = await render(NotesPanelFixture, {
    props: { notes: [], bookmarks: [{ id: 'b1', kind: 'pdf', pageNumber: 3 }], events },
  })

  expect(container.querySelector('.empty')).toBeNull()
  expect(container.querySelector('.section-heading')?.textContent).toContain('Bookmarks (1)')
})
