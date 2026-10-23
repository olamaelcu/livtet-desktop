import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/callout/callout.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/spinner/spinner.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

import CatalogDetailPageFixture from './fixtures/CatalogDetailPageFixture.svelte'

function feed(title: string, overrides: Record<string, unknown> = {}) {
  return {
    title,
    identifier: null,
    navigation: [],
    publications: [],
    next_href: null,
    has_search: false,
    ...overrides,
  }
}

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

/** What each command does when called. */
let onFeed: () => Promise<unknown>
let onSearch: () => Promise<unknown>

beforeEach(() => {
  onFeed = async () => feed('Standard Ebooks')
  onSearch = async () => feed('Search results')
  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'opds_feed':
        return onFeed()
      case 'opds_search':
        return onSearch()
      case 'opds_page':
        return onFeed()
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

function heading(container: HTMLElement): string | null {
  return container.querySelector('h2')?.textContent?.trim() ?? null
}

function toolbarButtons(container: HTMLElement): string[] {
  return [...container.querySelectorAll<WaButton>('wa-button')].map(
    (n) => n.textContent?.trim() ?? '',
  )
}

test('shows a spinner while the feed loads', async () => {
  onFeed = () => new Promise(() => {})

  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => container.querySelector('wa-spinner')).not.toBeNull()
  expect(heading(container)).toBeNull()
})

test('renders the feed title once it arrives', async () => {
  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => heading(container)).toBe('Standard Ebooks')
  expect(invoke).toHaveBeenCalledWith('opds_feed', { id: 'cat-standard-ebooks' })
})

test('surfaces a refused feed as an error rather than an empty catalog', async () => {
  onFeed = async () => {
    throw { Network: 'catalog unreachable' }
  }

  const { container } = await render(CatalogDetailPageFixture)

  // The feed query retries once before surfacing the error; allow for it.
  await expect.poll(() => container.querySelector('wa-callout'), { timeout: 5_000 }).not.toBeNull()
  expect(container.querySelector('wa-callout')?.textContent?.trim()).toBe('catalog unreachable')
  expect(heading(container)).toBeNull()
})

// The search box is only offered when the feed advertises search, so a
// catalog without it must not show one.
test('hides the search box for a catalog that does not advertise search', async () => {
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  expect(container.querySelector('wa-input')).toBeNull()
  expect(toolbarButtons(container)).not.toContain('Search')
})

test('offers search when the feed advertises it', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })

  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => container.querySelector('wa-input')).not.toBeNull()
  expect(toolbarButtons(container)).toContain('Search')
})

// Root is the initial state, so there is nothing to go back to.
test('does not offer "back to root" before navigating or searching', async () => {
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  expect(toolbarButtons(container)).not.toContain('Back to root')
})

test('a blank search is not submitted', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => container.querySelector('wa-input')).not.toBeNull()

  const input = container.querySelector('wa-input') as WaInput
  await input.updateComplete
  input.value = '   '
  input.dispatchEvent(new Event('input', { bubbles: true }))

  const search = [...container.querySelectorAll<WaButton>('wa-button')].find(
    (n) => n.textContent?.trim() === 'Search',
  )
  await search?.updateComplete
  search?.click()

  // Still the feed title, and no search command was issued.
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')
  expect(toolbarButtons(container)).not.toContain('Back to root')
})

async function submitSearch(container: HTMLElement, query: string) {
  const input = container.querySelector('wa-input') as WaInput
  await input.updateComplete
  input.value = query
  input.dispatchEvent(new Event('input', { bubbles: true }))

  const search = [...container.querySelectorAll<WaButton>('wa-button')].find(
    (n) => n.textContent?.trim() === 'Search',
  )
  await search?.updateComplete
  search?.click()
}

// `active`, `loading` and `failure` all switch source once a search is
// submitted, so the results feed must replace the browse feed on screen.
test('a submitted search replaces the feed with the results', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  await submitSearch(container, 'dickens')

  await expect.poll(() => heading(container)).toBe('Search results')
  expect(invoke).toHaveBeenCalledWith('opds_search', {
    id: 'cat-standard-ebooks',
    query: 'dickens',
  })
})

test('the search query is trimmed before it is sent', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  await submitSearch(container, '  dickens  ')

  await expect.poll(() => heading(container)).toBe('Search results')
  expect(invoke).toHaveBeenCalledWith('opds_search', {
    id: 'cat-standard-ebooks',
    query: 'dickens',
  })
})

test('a feed with publications renders their cards', async () => {
  onFeed = async () =>
    feed('Standard Ebooks', {
      publications: [
        {
          title: 'Moby Dick',
          identifier: 'moby',
          authors: ['Herman Melville'],
          summary: 'A whale.',
          cover_url: null,
          language: 'English',
          publisher: 'Standard Ebooks',
          published: '2020',
          acquisition_href: null,
          acquisition_type: null,
        },
      ],
    })
  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => container.textContent).toContain('Moby Dick')
  expect(container.textContent).toContain('Herman Melville')
  expect(container.textContent).toContain('English')
})

test('an empty feed says it has no publications', async () => {
  onFeed = async () => feed('Standard Ebooks')
  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => container.textContent).toContain('no publications')
})

test('a paged feed offers the next page', async () => {
  onFeed = async () => feed('Standard Ebooks', { next_href: 'page-2' })
  const { container } = await render(CatalogDetailPageFixture)

  await expect.poll(() => heading(container)).toBe('Standard Ebooks')
  await expect.poll(() => container.textContent).toContain('Next page')
  const next = [...container.querySelectorAll('wa-button')].find((button) =>
    button.textContent?.includes('Next page'),
  )
  expect(next, 'a next-page button').not.toBeUndefined()
  next?.click()

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'opds_page')).toBe(true)
})

test('searching offers a way back to the catalogue root', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  await submitSearch(container, 'dickens')
  await expect.poll(() => heading(container)).toBe('Search results')
  expect(toolbarButtons(container)).toContain('Back to root')

  const back = [...container.querySelectorAll<WaButton>('wa-button')].find(
    (n) => n.textContent?.trim() === 'Back to root',
  )
  await back?.updateComplete
  back?.click()

  await expect.poll(() => heading(container)).toBe('Standard Ebooks')
  expect(toolbarButtons(container)).not.toContain('Back to root')
})

// A failed search must surface as an error, not fall back to showing the
// browse feed as though the search had returned it.
test('a refused search shows an error rather than the browse feed', async () => {
  onFeed = async () => feed('Standard Ebooks', { has_search: true })
  onSearch = async () => {
    throw { Network: 'search endpoint refused' }
  }
  const { container } = await render(CatalogDetailPageFixture)
  await expect.poll(() => heading(container)).toBe('Standard Ebooks')

  await submitSearch(container, 'dickens')

  // The search query retries once before surfacing the error; allow for it.
  await expect.poll(() => container.querySelector('wa-callout'), { timeout: 5_000 }).not.toBeNull()
  expect(container.querySelector('wa-callout')?.textContent?.trim()).toBe('search endpoint refused')
  expect(heading(container)).toBeNull()
})
