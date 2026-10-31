// The virtual-book drawer is the only way a catalog-only book comes into being,
// and everything load-bearing about it is client-side: which fields gate
// submission, how blank optionals collapse to null, and how a refused command is
// reported. Driving the real WebAwesome controls is the only way to see the
// disabled state of the submit button, so this is a browser test.
import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/option/option.js'
import '@awesome.me/webawesome/dist/components/select/select.js'
import '@awesome.me/webawesome/dist/components/textarea/textarea.js'
import { QueryClient } from '@tanstack/svelte-query'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import { searchKeys } from '$lib/query/keys'

const { invoke, toast } = vi.hoisted(() => ({
  invoke: vi.fn(),
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}))
vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke,
}))
vi.mock('svelte-sonner', () => ({ toast }))

import VirtualBookDrawerFixture from './fixtures/VirtualBookDrawerFixture.svelte'

type WaControl = HTMLElement & { value: string; label?: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

/** One edition as `search_biblio_editions` reports it. */
interface IndexEdition {
  uri: string | null
  title: string
  cover_image_url: string | null
  published_year: number | null
  language: string | null
  authors: { name: string; role: string }[]
  identifiers: { resource: string; uri: string }[]
}

const FILTER_OPTIONS = {
  formats: [{ id: 'fmt-epub', label: 'EPUB', flag_emoji: null, logo_url: null }],
  languages: [{ id: 'lang-en', label: 'English', flag_emoji: null, logo_url: null }],
  tags: [],
  genres: [],
  subjects: [],
  publishers: [],
  authors: [],
}

function indexEdition(overrides: Partial<IndexEdition> = {}): IndexEdition {
  return {
    uri: 'at://did:web:biblio/community.lexicon.book.edition/dune',
    title: 'Dune',
    cover_image_url: null,
    published_year: null,
    language: null,
    authors: [{ name: 'Frank Herbert', role: 'aut' }],
    identifiers: [],
    ...overrides,
  }
}

type Handler = (args: Record<string, unknown>) => unknown

/** Route each command to a handler; anything unrouted fails loudly. */
function serve(overrides: Record<string, Handler> = {}) {
  const handlers: Record<string, Handler> = {
    filter_options: () => FILTER_OPTIONS,
    create_virtual_book: () => null,
    ...overrides,
  }
  invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
    const handler = handlers[command]
    if (!handler) throw new Error(`unexpected command: ${command}`)
    return handler(args)
  })
}

const invalidateQueries = vi.spyOn(QueryClient.prototype, 'invalidateQueries')

beforeEach(() => {
  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  toast.warning.mockReset()
  invalidateQueries.mockClear()
  serve()
})

function actionButtons(container: HTMLElement): WaButton[] {
  return [...container.querySelectorAll<WaButton>('.actions wa-button')]
}

function createButton(container: HTMLElement): WaButton {
  const found = actionButtons(container)[1]
  expect(found, 'the manual panel offers a create action').not.toBeUndefined()
  return found
}

function cancelButton(container: HTMLElement): WaButton {
  const found = actionButtons(container)[0]
  expect(found, 'the manual panel offers a cancel action').not.toBeUndefined()
  return found
}

/** Svelte sets `label` as a property on wa-input, so the attribute never lands
 * and an attribute selector matches nothing. */
function controlByLabel(container: HTMLElement, label: string): WaControl {
  const controls = [
    ...container.querySelectorAll<WaControl>(
      '.panel wa-input, .panel wa-select, .panel wa-textarea',
    ),
  ]
  const found = controls.find((node) => node.label === label)
  expect(found, `a "${label}" control`).not.toBeUndefined()
  return found as WaControl
}

function authorRows(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>('.author-row')]
}

function authorName(row: HTMLElement): WaControl {
  return row.querySelector('wa-input') as WaControl
}

function addAuthorButton(container: HTMLElement): WaButton {
  const found = container.querySelector<WaButton>('.authors > wa-button')
  expect(found, 'the author fieldset offers an add action').not.toBeUndefined()
  return found as WaButton
}

async function typeInto(control: WaControl, value: string) {
  await control.updateComplete
  control.value = value
  control.dispatchEvent(new Event('input', { bubbles: true }))
  await control.updateComplete
}

async function choose(control: WaControl, value: string) {
  await control.updateComplete
  control.value = value
  control.dispatchEvent(new Event('change', { bubbles: true }))
  await control.updateComplete
}

async function click(button: WaButton) {
  await button.updateComplete
  button.click()
}

function optionLabels(control: WaControl): string[] {
  return [...control.querySelectorAll('wa-option')].map((node) => node.textContent?.trim() ?? '')
}

/** The `input` payload of the first `create_virtual_book` call. */
async function createdInput(): Promise<Record<string, unknown>> {
  await expect
    .poll(() => invoke.mock.calls.some(([command]) => command === 'create_virtual_book'))
    .toBe(true)
  const call = invoke.mock.calls.find(([command]) => command === 'create_virtual_book')
  // `invoke` is untyped here; the command's whole argument is the book payload.
  const args = call?.[1] as { input: Record<string, unknown> } | undefined
  expect(args?.input, 'create_virtual_book got an input').not.toBeUndefined()
  return args?.input ?? {}
}

async function fillRequired(container: HTMLElement, title = 'Dune') {
  await typeInto(controlByLabel(container, 'Title'), title)
  await typeInto(authorName(authorRows(container)[0]), 'Frank Herbert')
}

function searchInput(container: HTMLElement): WaControl {
  return container.querySelector('.search-row wa-input') as WaControl
}

function searchButton(container: HTMLElement): WaButton {
  const found = container.querySelector<WaButton>('.search-row wa-button')
  expect(found, 'the search panel offers a search action').not.toBeUndefined()
  return found as WaButton
}

function results(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>('.result')]
}

async function searchFor(container: HTMLElement, query: string) {
  await typeInto(searchInput(container), query)
  await click(searchButton(container))
}

test('creating a virtual book sends every trimmed field, closes, and refreshes search', async () => {
  const events: string[] = []
  const { container } = await render(VirtualBookDrawerFixture, { props: { events } })

  await expect
    .poll(() => optionLabels(controlByLabel(container, 'Format')))
    .toEqual(['None', 'EPUB'])

  await typeInto(controlByLabel(container, 'Title'), '  Dune  ')
  await typeInto(authorName(authorRows(container)[0]), '  Frank Herbert  ')
  await typeInto(
    controlByLabel(container, 'Cover image URL'),
    '  https://covers.example/dune.jpg  ',
  )
  await typeInto(controlByLabel(container, 'Publisher'), '  Chilton Books  ')
  await choose(controlByLabel(container, 'Language'), 'lang-en')
  await choose(controlByLabel(container, 'Format'), 'fmt-epub')
  await typeInto(
    controlByLabel(container, 'Identifiers'),
    '9780441013593, isbn:urn:isbn:1\n\n  ,  ',
  )
  await typeInto(controlByLabel(container, 'Description'), '  A desert planet.  ')
  await typeInto(controlByLabel(container, 'Notes'), '  First edition.  ')

  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))

  expect(await createdInput()).toEqual({
    title: 'Dune',
    authors: [{ name: 'Frank Herbert', role: 'aut' }],
    publisher: 'Chilton Books',
    language_id: 'lang-en',
    format_id: 'fmt-epub',
    identifiers: ['9780441013593', 'isbn:urn:isbn:1'],
    description: 'A desert planet.',
    notes: 'First edition.',
    cover_image_url: 'https://covers.example/dune.jpg',
  })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Added “Dune” as a virtual book')
  await expect.poll(() => events).toEqual(['close'])
  expect(invalidateQueries).toHaveBeenCalledWith({ queryKey: searchKeys.all })

  // The drawer resets itself, so a second create needs the fields filled again.
  await expect.poll(() => createButton(container).disabled).toBe(true)
  expect(authorRows(container)).toHaveLength(1)
})

test('creating needs a title and at least one author name', async () => {
  const { container } = await render(VirtualBookDrawerFixture)

  await expect.poll(() => createButton(container).disabled).toBe(true)

  // The disabled action must not reach the command, however it is clicked.
  await click(createButton(container))
  expect(invoke).not.toHaveBeenCalledWith('create_virtual_book', expect.anything())

  await typeInto(controlByLabel(container, 'Title'), 'Dune')
  expect(createButton(container).disabled).toBe(true)

  await typeInto(authorName(authorRows(container)[0]), '   ')
  expect(createButton(container).disabled).toBe(true)

  await typeInto(authorName(authorRows(container)[0]), 'Frank Herbert')
  await expect.poll(() => createButton(container).disabled).toBe(false)

  await typeInto(controlByLabel(container, 'Title'), '   ')
  await expect.poll(() => createButton(container).disabled).toBe(true)
})

test('untouched optional fields are sent as null with no identifiers', async () => {
  const { container } = await render(VirtualBookDrawerFixture)
  await fillRequired(container)

  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))

  expect(await createdInput()).toEqual({
    title: 'Dune',
    authors: [{ name: 'Frank Herbert', role: 'aut' }],
    publisher: null,
    language_id: null,
    format_id: null,
    identifiers: [],
    description: null,
    notes: null,
    cover_image_url: null,
  })
})

test('an empty cover url is null, and a bare filename is not validated away', async () => {
  const { container } = await render(VirtualBookDrawerFixture)
  await fillRequired(container)

  await typeInto(controlByLabel(container, 'Cover image URL'), '   ')
  await typeInto(controlByLabel(container, 'Identifiers'), '  ,\n ,  ')
  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))
  const blank = await createdInput()
  expect(blank.cover_image_url).toBeNull()
  expect(blank.identifiers).toEqual([])
})

test('a second author row can be added, retyped, and the first removed', async () => {
  const { container } = await render(VirtualBookDrawerFixture)
  await expect.poll(() => authorRows(container)).toHaveLength(1)

  const onlyRemove = authorRows(container)[0].querySelector<WaButton>('wa-button')
  expect(onlyRemove).not.toBeUndefined()
  await onlyRemove?.updateComplete
  expect(onlyRemove?.disabled).toBe(true)

  await click(addAuthorButton(container))
  await expect.poll(() => authorRows(container)).toHaveLength(2)

  await typeInto(authorName(authorRows(container)[0]), 'Frank Herbert')
  await typeInto(authorName(authorRows(container)[1]), 'Brian Herbert')
  const roleSelect = authorRows(container)[1].querySelector('wa-select') as WaControl
  await choose(roleSelect, 'edt')

  const removeFirst = authorRows(container)[0].querySelector<WaButton>('wa-button')
  await removeFirst?.updateComplete
  expect(removeFirst?.disabled).toBe(false)
  await removeFirst?.click()

  await expect.poll(() => authorRows(container)).toHaveLength(1)

  await typeInto(controlByLabel(container, 'Title'), 'Dune: House Atreides')
  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))

  expect((await createdInput()).authors).toEqual([{ name: 'Brian Herbert', role: 'edt' }])
})

test('a refused create reports the message and keeps the drawer open', async () => {
  serve({
    create_virtual_book: () => {
      throw new Error('duplicate edition')
    },
  })
  const events: string[] = []
  const { container } = await render(VirtualBookDrawerFixture, { props: { events } })
  await fillRequired(container)

  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))

  await expect.poll(() => toast.error).toHaveBeenCalledWith('duplicate edition')
  expect(events).toEqual([])
  expect(toast.success).not.toHaveBeenCalled()
  expect(invalidateQueries).not.toHaveBeenCalled()
  // `busy` is cleared, so the drawer is usable again rather than stuck creating.
  await expect.poll(() => createButton(container).disabled).toBe(false)
  expect(createButton(container).textContent).toBe('Create virtual book')
})

test('a rejection that is not an Error still reports something', async () => {
  serve({
    create_virtual_book: () => {
      throw { code: 'db', message: 'ignored' }
    },
  })
  const { container } = await render(VirtualBookDrawerFixture)
  await fillRequired(container)

  await expect.poll(() => createButton(container).disabled).toBe(false)
  await click(createButton(container))

  await expect.poll(() => toast.error).toHaveBeenCalledWith('Could not create the virtual book')
})

test('cancel closes without creating anything', async () => {
  const events: string[] = []
  const { container } = await render(VirtualBookDrawerFixture, { props: { events } })
  await fillRequired(container)
  await expect.poll(() => createButton(container).disabled).toBe(false)

  await click(cancelButton(container))

  expect(events).toEqual(['close'])
  expect(invoke).not.toHaveBeenCalledWith('create_virtual_book', expect.anything())
})

test('filter options fill the pickers each time the drawer opens', async () => {
  const { container, rerender } = await render(VirtualBookDrawerFixture, { open: false })

  expect(invoke).not.toHaveBeenCalledWith('filter_options')
  await expect.poll(() => optionLabels(controlByLabel(container, 'Format'))).toEqual(['None'])

  await rerender({ open: true })

  await expect
    .poll(() => optionLabels(controlByLabel(container, 'Format')))
    .toEqual(['None', 'EPUB'])
  expect(optionLabels(controlByLabel(container, 'Language'))).toEqual(['None', 'English'])
})

test('filter options arriving after the drawer closes are discarded', async () => {
  const pending = Promise.withResolvers<typeof FILTER_OPTIONS>()
  serve({ filter_options: () => pending.promise })
  const { container, rerender } = await render(VirtualBookDrawerFixture, { props: { open: true } })

  await expect
    .poll(() => invoke.mock.calls.filter(([command]) => command === 'filter_options'))
    .toHaveLength(1)

  await rerender({ open: false })
  pending.resolve(FILTER_OPTIONS)

  // The load has no observable effect once abandoned, so drain its promise chain
  // before checking that it filled nothing.
  for (let turn = 0; turn < 10; turn += 1) await Promise.resolve()

  expect(optionLabels(controlByLabel(container, 'Format'))).toEqual(['None'])
  expect(invoke.mock.calls.filter(([command]) => command === 'filter_options')).toHaveLength(1)
})

test('filter options that fail to load leave the pickers empty without a fuss', async () => {
  serve({
    filter_options: () => {
      throw new Error('index offline')
    },
  })
  const { container } = await render(VirtualBookDrawerFixture)

  await expect
    .poll(() => invoke.mock.calls.filter(([command]) => command === 'filter_options'))
    .toHaveLength(1)
  for (let turn = 0; turn < 10; turn += 1) await Promise.resolve()

  expect(optionLabels(controlByLabel(container, 'Format'))).toEqual(['None'])
  expect(optionLabels(controlByLabel(container, 'Language'))).toEqual(['None'])
  expect(toast.error).not.toHaveBeenCalled()
  expect(container.querySelector('wa-callout')).toBeNull()
})

test('a search lists each edition with its year, authors, identifiers, and cover', async () => {
  serve({
    search_biblio_editions: () => [
      indexEdition({
        title: 'Dune',
        published_year: 1965,
        authors: [
          { name: 'Frank Herbert', role: 'aut' },
          { name: 'Neil Gaiman', role: 'edt' },
        ],
        identifiers: [{ resource: 'isbn', uri: 'urn:isbn:9780441013593' }],
        cover_image_url: 'https://covers.example/dune.jpg',
      }),
      indexEdition({ uri: null, title: 'Dune Messiah', authors: [], identifiers: [] }),
    ],
  })
  const { container } = await render(VirtualBookDrawerFixture)

  await searchFor(container, '  dune  ')

  await expect.poll(() => results(container)).toHaveLength(2)
  expect(invoke).toHaveBeenCalledWith('search_biblio_editions', { query: 'dune', limit: 20 })

  const [first, second] = results(container)
  expect(first.textContent).toContain('Dune')
  expect(first.textContent).toContain('1965')
  expect(first.textContent).toContain('Frank Herbert, Neil Gaiman')
  expect(first.querySelector('wa-badge')?.textContent).toBe('isbn')
  expect(second.querySelector('img')).toBeNull()

  // A cover that 404s hides itself instead of leaving a broken image.
  const cover = first.querySelector('img')
  expect(cover?.getAttribute('src')).toBe('https://covers.example/dune.jpg')
  cover?.dispatchEvent(new Event('error'))
  expect(cover?.style.display).toBe('none')
})

test('a blank query searches nothing, even from the form', async () => {
  const { container } = await render(VirtualBookDrawerFixture)

  await typeInto(searchInput(container), '   ')
  await expect.poll(() => searchButton(container).disabled).toBe(true)

  const form = container.querySelector('.search-row') as HTMLFormElement
  form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  await click(searchButton(container))

  expect(invoke).not.toHaveBeenCalledWith('search_biblio_editions', expect.anything())
})

test('submitting the search form runs the search', async () => {
  serve({ search_biblio_editions: () => [indexEdition()] })
  const { container } = await render(VirtualBookDrawerFixture)

  await typeInto(searchInput(container), 'dune')
  const form = container.querySelector('.search-row') as HTMLFormElement
  form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))

  await expect.poll(() => results(container)).toHaveLength(1)
  expect(invoke).toHaveBeenCalledWith('search_biblio_editions', { query: 'dune', limit: 20 })
})

test('a failed search replaces the earlier results with the message', async () => {
  let failing = false
  serve({
    search_biblio_editions: () => {
      if (failing) throw new Error('index offline')
      return [indexEdition()]
    },
  })
  const { container } = await render(VirtualBookDrawerFixture)
  await searchFor(container, 'dune')
  await expect.poll(() => results(container)).toHaveLength(1)

  failing = true
  await searchFor(container, 'dune messiah')

  await expect
    .poll(() => container.querySelector('wa-callout')?.textContent)
    .toContain('index offline')
  expect(results(container)).toHaveLength(0)
})

test('a search rejection that is not an Error still reports something', async () => {
  serve({
    search_biblio_editions: () => {
      throw { code: 'index', message: 'ignored' }
    },
  })
  const { container } = await render(VirtualBookDrawerFixture)

  await searchFor(container, 'dune')

  await expect
    .poll(() => container.querySelector('wa-callout')?.textContent)
    .toContain('Could not search the book index')
})

test('a search with no matches says so', async () => {
  serve({ search_biblio_editions: () => [] })
  const { container } = await render(VirtualBookDrawerFixture)

  await searchFor(container, 'nothing at all')

  await expect.poll(() => container.textContent).toContain('No editions matched that search.')
  expect(results(container)).toEqual([])
})

test('a search in flight reports progress and is not started twice', async () => {
  const pending = Promise.withResolvers<IndexEdition[]>()
  serve({ search_biblio_editions: () => pending.promise })
  const { container } = await render(VirtualBookDrawerFixture)

  await searchFor(container, 'dune')

  await expect.poll(() => container.textContent).toContain('Searching the book index…')
  expect(searchButton(container).textContent).toBe('Searching…')
  expect(searchButton(container).disabled).toBe(true)

  await click(searchButton(container))
  expect(
    invoke.mock.calls.filter(([command]) => command === 'search_biblio_editions'),
  ).toHaveLength(1)

  pending.resolve([indexEdition()])
  await expect.poll(() => results(container)).toHaveLength(1)
  expect(
    invoke.mock.calls.filter(([command]) => command === 'search_biblio_editions'),
  ).toHaveLength(1)
})

test('adding an edition from the index creates a virtual book from it', async () => {
  serve({
    search_biblio_editions: () => [
      indexEdition({
        title: 'Dune',
        published_year: 1965,
        authors: [
          { name: 'Frank Herbert', role: 'aut' },
          { name: 'Brian Herbert', role: 'edt' },
        ],
        identifiers: [
          { resource: 'isbn', uri: 'urn:isbn:9780441013593' },
          { resource: 'asin', uri: 'urn:asin:B000000000' },
        ],
        cover_image_url: 'https://covers.example/dune.jpg',
      }),
    ],
  })
  const events: string[] = []
  const { container } = await render(VirtualBookDrawerFixture, { props: { events } })

  await searchFor(container, 'dune')
  await expect.poll(() => results(container)).toHaveLength(1)

  const addButton = results(container)[0].querySelector<WaButton>('wa-button')
  expect(addButton).not.toBeUndefined()
  await addButton?.updateComplete
  expect(addButton?.disabled).toBe(false)
  await addButton?.click()

  expect(await createdInput()).toEqual({
    title: 'Dune',
    authors: [
      { name: 'Frank Herbert', role: 'aut' },
      { name: 'Brian Herbert', role: 'edt' },
    ],
    publisher: null,
    language_id: null,
    format_id: null,
    identifiers: ['urn:isbn:9780441013593', 'urn:asin:B000000000'],
    description: null,
    notes: null,
    cover_image_url: 'https://covers.example/dune.jpg',
  })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Added “Dune” as a virtual book')
  await expect.poll(() => events).toEqual(['close'])
})
