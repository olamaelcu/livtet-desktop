// Enrichment is client-side glue over two commands (ADR-0046): the drawer
// previews, renders the diff, and writes it under the mode the user picked.
// What matters here is only observable in the rendered drawer — whether the
// action is offered at all, which side of a conflict a mode marks as winning,
// and what apply then invalidates — so this is a browser test.
import { QueryClient } from '@tanstack/svelte-query'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import type { EnrichmentPreview, EnrichmentSummary } from '$lib/library/enrich'
import { catalogKeys, searchKeys } from '$lib/query/keys'
import type { EditionDetail } from '$lib/search'

const { convertFileSrc, invoke, toast } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  // Typed so every assertion below reads routed arguments without casting.
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))
vi.mock('svelte-sonner', () => ({ toast }))

import EditionDetailDrawerFixture from './fixtures/EditionDetailDrawerFixture.svelte'

const EDITION_ID = '01hzzzzzzzzzzzzzzzzzzzzzzz'
const NO_IDENTIFIERS_REASON =
  'Add an identifier (such as an ISBN) to this edition before enriching it.'

/** One edition as `get_edition_detail` reports it. */
function detail(overrides: Partial<EditionDetail> = {}): EditionDetail {
  return {
    id: EDITION_ID,
    work_id: '01hwwwwwwwwwwwwwwwwwwwwwww',
    title: 'Positive Obsession',
    published_date: '2016-01-01',
    format: 'EPUB',
    language_code: 'en',
    language_name: 'English',
    notes: null,
    description: 'A description.',
    authors: [{ name: 'Susana M. Morris', role: 'aut', role_label: 'Author' }],
    publishers: [],
    identifiers: [{ kind: 'isbn', value: '9780802124675' }],
    file: null,
    created_at: '2026-01-01',
    updated_at: null,
    ...overrides,
  }
}

/** A diff that touches every section the review screen renders. */
const PREVIEW: EnrichmentPreview = {
  sources: ['openlibrary'],
  added: [{ field: 'publisher', value: 'Grove Press' }],
  unioned: [{ field: 'subjects', values: ['Feminism', 'American literature'] }],
  conflicts: [
    {
      field: 'title',
      current: 'Positive Obsession',
      incoming: 'Positive Obsession: A Life',
    },
  ],
  found: [{ kind: 'isbn', value: 'urn:isbn:9780802124675' }],
  cover: { mime: 'image/jpeg', storable: true },
}

const SUMMARY: EnrichmentSummary = {
  edition_id: EDITION_ID,
  sources: ['openlibrary'],
  mode: 'union',
  fields_added: 1,
  fields_overwritten: 0,
  fields_unioned: 1,
  identifiers_persisted: 1,
  cover_stored: true,
}

type Handler = (args: Record<string, unknown>) => unknown

/** Route each command to a handler; anything unrouted fails loudly. */
function serve(overrides: Record<string, Handler> = {}) {
  const handlers: Record<string, Handler> = {
    get_edition_detail: () => detail(),
    preview_enrichment: () => PREVIEW,
    apply_enrichment: () => SUMMARY,
    ...overrides,
  }
  invoke.mockImplementation(async (command, args = {}) => {
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler(args)
  })
}

const invalidateQueries = vi.spyOn(QueryClient.prototype, 'invalidateQueries')

beforeEach(() => {
  invoke.mockReset()
  toast.info.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invalidateQueries.mockClear()
  serve()
})

afterEach(() => {
  // wa-drawer escapes the render container like wa-dialog does.
  for (const drawer of document.querySelectorAll('wa-drawer')) drawer.remove()
})

/**
 * The components are declared in `app.wa.js`, which the browser project does
 * not load, so `wa-button` is a plain custom element: its label is its light-DOM
 * text and Svelte writes boolean attributes as the strings `"true"`/`"false"`
 * (`disabled="false"` is an enabled button).
 */
function button(container: HTMLElement, label: string): HTMLElement {
  const found = [...container.querySelectorAll('wa-button')].find(
    (node) => node.textContent?.trim() === label,
  )
  expect(found, `a "${label}" button`).not.toBeUndefined()
  return found as HTMLElement
}

function section(container: HTMLElement, title: string): HTMLElement {
  const found = [...container.querySelectorAll('section')].find(
    (node) => node.querySelector('h3')?.textContent?.trim() === title,
  )
  expect(found, `a "${title}" section`).not.toBeUndefined()
  return found as HTMLElement
}

function commandArgs(command: string): Record<string, unknown>[] {
  return invoke.mock.calls.filter(([name]) => name === command).map(([, args]) => args ?? {})
}

function callsContaining(queryKey: readonly unknown[]): boolean {
  return invalidateQueries.mock.calls.some(([filters]) => {
    if (!(filters && typeof filters === 'object' && 'queryKey' in filters)) return false
    return JSON.stringify(filters.queryKey) === JSON.stringify(queryKey)
  })
}

/** Render the drawer and wait for the catalog record to land. */
async function openDrawer(): Promise<HTMLElement> {
  const { container } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent?.trim())
    .toBe('Positive Obsession')
  return container
}

/** Click Enrich and wait for the review screen to replace the record. */
async function previewDrawer(container: HTMLElement): Promise<void> {
  button(container, 'Enrich').click()
  await expect.poll(() => sectionTitles(container)).toContain('Sources')
}

function sectionTitles(container: HTMLElement): string[] {
  return [...container.querySelectorAll('section h3')].map((node) => node.textContent?.trim() ?? '')
}

test('an edition without identifiers cannot enrich, and the reason explains why', async () => {
  serve({ get_edition_detail: () => detail({ identifiers: [] }) })
  const container = await openDrawer()

  const enrich = button(container, 'Enrich')
  await expect.poll(() => enrich.getAttribute('disabled')).toBe('true')
  expect(enrich.getAttribute('title')).toBe(NO_IDENTIFIERS_REASON)

  enrich.click()
  // Nothing was looked up: the control never dispatches.
  expect(commandArgs('preview_enrichment')).toEqual([])
  expect(sectionTitles(container)).not.toContain('Sources')
})

test('previewing renders sources, added, unioned, conflicts, identifiers, and cover', async () => {
  const container = await openDrawer()
  await previewDrawer(container)

  expect(commandArgs('preview_enrichment')).toEqual([{ editionId: EDITION_ID }])
  expect(section(container, 'Sources').textContent).toContain('openlibrary')

  const added = section(container, 'Added')
  expect(added.textContent).toContain('publisher')
  expect(added.textContent).toContain('Grove Press')

  const unioned = section(container, 'Unioned')
  expect(unioned.textContent).toContain('subjects')
  expect(unioned.textContent).toContain('Feminism, American literature')

  const conflicts = section(container, 'Conflicts')
  expect(conflicts.querySelector('[data-side="current"]')?.textContent).toContain(
    'Positive Obsession',
  )
  expect(conflicts.querySelector('[data-side="incoming"]')?.textContent).toContain(
    'Positive Obsession: A Life',
  )

  // The stored identifier loses its `urn:isbn:` prefix on the way in.
  const found = section(container, 'Found identifiers')
  expect(found.textContent).toContain('9780802124675')

  expect(section(container, 'Cover').textContent).toContain('image/jpeg')
  expect(section(container, 'Cover').textContent).toContain('will be saved')
})

test('the mode control marks the winning side, and apply sends that mode', async () => {
  const container = await openDrawer()
  await previewDrawer(container)

  const current = () => section(container, 'Conflicts').querySelector('[data-side="current"]')
  const incoming = () => section(container, 'Conflicts').querySelector('[data-side="incoming"]')

  // Union is the default and keeps what the catalog already has.
  await expect.poll(() => button(container, 'Union').getAttribute('aria-pressed')).toBe('true')
  expect(current()?.classList.contains('wins')).toBe(true)
  expect(incoming()?.classList.contains('loses')).toBe(true)

  button(container, 'Overwrite').click()
  await expect.poll(() => incoming()?.classList.contains('wins')).toBe(true)
  expect(current()?.classList.contains('loses')).toBe(true)
  expect(button(container, 'Overwrite').getAttribute('aria-pressed')).toBe('true')

  button(container, 'Apply').click()

  await expect
    .poll(() => commandArgs('apply_enrichment'))
    .toEqual([{ editionId: EDITION_ID, mode: 'overwrite' }])
})

test('Cancel discards the preview and writes nothing', async () => {
  const container = await openDrawer()
  await previewDrawer(container)

  button(container, 'Cancel').click()

  await expect
    .poll(() => container.querySelector('h2.title')?.textContent?.trim())
    .toBe('Positive Obsession')
  expect(sectionTitles(container)).not.toContain('Sources')
  expect(commandArgs('apply_enrichment')).toEqual([])
  expect(invalidateQueries).not.toHaveBeenCalled()
})

test('an expired preview surfaces the backend message and keeps the diff', async () => {
  serve({
    apply_enrichment: () =>
      Promise.reject({ Invalid: { message: 'preview expired; run Enrich again' } }),
  })
  const container = await openDrawer()
  await previewDrawer(container)

  button(container, 'Apply').click()

  await expect
    .poll(() => toast.error.mock.calls.map(([message]) => message))
    .toEqual(['Enrichment failed: preview expired; run Enrich again'])
  // The review is still on screen so the retry costs one click.
  expect(sectionTitles(container)).toContain('Sources')
})

test('apply returns to the record, reports the summary, and invalidates the detail key', async () => {
  const container = await openDrawer()
  await previewDrawer(container)

  button(container, 'Apply').click()

  await expect
    .poll(() => container.querySelector('h2.title')?.textContent?.trim())
    .toBe('Positive Obsession')
  expect(sectionTitles(container)).not.toContain('Sources')
  await expect
    .poll(() => toast.success.mock.calls.map(([message]) => message))
    .toEqual(['Added 1 field, extended 1 list, linked 1 identifier.'])
  expect(callsContaining(catalogKeys.editionDetail(EDITION_ID))).toBe(true)
  // Fields changed, so the search results the drawer was opened from are stale.
  expect(callsContaining(searchKeys.all)).toBe(true)
})

test('a preview never survives another book or a reopened drawer', async () => {
  const { container, rerender } = await render(EditionDetailDrawerFixture)
  await expect
    .poll(() => container.querySelector('h2.title')?.textContent?.trim())
    .toBe('Positive Obsession')
  button(container, 'Enrich').click()
  await expect.poll(() => sectionTitles(container)).toContain('Sources')

  // The drawer is reused across books; the diff belonged to the previous one.
  await rerender({ editionId: '01hotherbook0000000000000', open: true })
  await expect.poll(() => sectionTitles(container)).not.toContain('Sources')

  // Closing and reopening starts from the record again, never a stale diff.
  button(container, 'Enrich').click()
  await expect.poll(() => sectionTitles(container)).toContain('Sources')
  await rerender({ editionId: '01hotherbook0000000000000', open: false })
  await rerender({ editionId: '01hotherbook0000000000000', open: true })
  await expect.poll(() => sectionTitles(container)).not.toContain('Sources')
})

test('a preview with nothing to offer keeps the record and says so', async () => {
  serve({
    preview_enrichment: () => ({
      sources: [],
      added: [],
      unioned: [],
      conflicts: [],
      found: [],
      cover: null,
    }),
  })
  const container = await openDrawer()

  button(container, 'Enrich').click()

  await expect.poll(() => toast.info).toHaveBeenCalled()
  expect(sectionTitles(container)).not.toContain('Sources')
  expect(container.querySelector('h2.title')?.textContent?.trim()).toBe('Positive Obsession')
})
