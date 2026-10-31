import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

// The route offers a CSV export through the native save dialog, and opens
// books in a second window; neither exists outside Tauri.
vi.mock('@tauri-apps/plugin-dialog', () => ({
  save: vi.fn(async () => null),
  open: vi.fn(async () => null),
}))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  WebviewWindow: vi.fn(),
  getCurrentWebviewWindow: vi.fn(() => ({ label: 'main' })),
}))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))
vi.mock('svelte-sonner', () => ({ toast }))

import LibraryPageFixture from './fixtures/LibraryPageFixture.svelte'

type Hit = {
  work_id: string
  edition_id: string | null
  title: string
  authors: string[]
  pub_date: string | null
  snippet_text: string | null
  kind: string
  has_file: boolean
}

function hit(title: string, overrides: Partial<Hit> = {}): Hit {
  return {
    work_id: `work-${title}`,
    edition_id: `ed-${title}`,
    title,
    authors: ['A. Writer'],
    pub_date: '2020',
    snippet_text: null,
    kind: 'edition',
    has_file: true,
    ...overrides,
  }
}

/** What the mocked backend returns from search_editions. */
let hits: Hit[]

const EMPTY_FILTER_OPTIONS = {
  formats: [],
  languages: [],
  tags: [],
  genres: [],
  subjects: [],
  publishers: [],
  authors: [],
}

beforeEach(() => {
  hits = []
  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'search_editions':
        return { hits, total: hits.length }
      case 'filter_options':
        return EMPTY_FILTER_OPTIONS
      case 'get_edition_covers':
        return []
      case 'search_typeahead':
        return []
      case 'delete_editions':
        return { deleted: 1, files_removed: 1, covers_removed: 0, skipped: [] }
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

function emptyState(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>('div.empty')
}

/** BookCard renders the title as the cover's figcaption. */
function cardTitles(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.card-wrap figcaption')]
    .map((node) => node.textContent?.trim())
    .filter((text): text is string => Boolean(text))
}

test('shows the empty state when the library has no editions', async () => {
  const { container } = await render(LibraryPageFixture)

  await expect.poll(() => emptyState(container)).not.toBeNull()

  expect(emptyState(container)?.textContent?.trim()).toBe('No books? No results.')
  expect(invoke).toHaveBeenCalledWith('search_editions', expect.anything())
  expect(toast.error).not.toHaveBeenCalled()
})

test('renders a card per edition once results arrive', async () => {
  hits = [hit('Capital'), hit('Debt')]

  const { container } = await render(LibraryPageFixture)

  await expect.poll(() => cardTitles(container).length).toBe(2)

  expect(cardTitles(container)).toEqual(['Capital', 'Debt'])
  // The empty state is the other arm of the same branch, so it must be gone
  // rather than merely hidden.
  expect(emptyState(container)).toBeNull()
  // The count lives in the floating pill now; the in-view part depends on
  // layout, so only the total is pinned here.
  expect(container.querySelector('.view-counter')?.textContent).toContain('2 books')
})

function actionBar(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>('[role="toolbar"][aria-label="Selection actions"]')
}

function buttonByLabel(container: HTMLElement, label: string): HTMLElement {
  // Actions render as `wa-button` custom elements; the confirm dialog uses
  // native buttons.
  const found = [...container.querySelectorAll<HTMLElement>('wa-button,button')].find((button) =>
    button.textContent?.includes(label),
  )
  expect(found, `a "${label}" button`).not.toBeUndefined()
  return found as HTMLElement
}

test('selection mode docks the action bar and counts selections', async () => {
  hits = [hit('Capital'), hit('Debt')]
  const { container } = await render(LibraryPageFixture)
  await expect.poll(() => cardTitles(container).length).toBe(2)

  expect(actionBar(container)).toBeNull()
  buttonByLabel(container, 'Select').click()
  await expect.poll(() => actionBar(container)?.textContent).toContain('0 selected')

  const [first] = [...container.querySelectorAll<HTMLButtonElement>('.card-wrap .card')]
  first?.click()
  await expect.poll(() => actionBar(container)?.textContent).toContain('1 selected')
})

test('deleting from selection calls delete_editions and toasts', async () => {
  hits = [hit('Capital')]
  const { container } = await render(LibraryPageFixture)
  await expect.poll(() => cardTitles(container).length).toBe(1)

  buttonByLabel(container, 'Select').click()
  await expect.poll(() => actionBar(container)).not.toBeNull()
  const [card] = [...container.querySelectorAll<HTMLButtonElement>('.card-wrap .card')]
  card?.click()
  await expect.poll(() => actionBar(container)?.textContent).toContain('1 selected')

  const bar = () => actionBar(container) as HTMLElement
  const barButton = (label: string) => {
    const found = [...bar().querySelectorAll<HTMLElement>('wa-button,button')].find((button) =>
      button.textContent?.includes(label),
    )
    expect(found, `an action-bar "${label}" button`).not.toBeUndefined()
    return found as HTMLElement
  }
  barButton('Delete').click()
  await expect
    .poll(() => container.querySelector('wa-dialog[label="Delete editions"]'))
    .not.toBeNull()
  expect(container.textContent).toContain('Delete 1 selected edition?')
  container.querySelector<HTMLButtonElement>('wa-dialog button.danger')?.click()

  await expect.poll(() => toast.success).toHaveBeenCalledWith('Deleted 1')
  expect(invoke).toHaveBeenCalledWith('delete_editions', { editionIds: ['ed-Capital'] })
})

// The empty state is guarded by `!editions.isPending && !editions.isError`, so
// a failed search must not render it: an error is not an empty library.
test('does not claim an empty library when the search fails', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'search_editions') throw new Error('index unavailable')
    if (command === 'filter_options') return EMPTY_FILTER_OPTIONS
    return []
  })

  const { container } = await render(LibraryPageFixture)

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'search_editions')).toBe(true)
  // Give the failed query a chance to settle before asserting on absence.
  await new Promise((resolve) => setTimeout(resolve, 50))

  expect(emptyState(container)).toBeNull()
  expect(cardTitles(container)).toEqual([])
})
