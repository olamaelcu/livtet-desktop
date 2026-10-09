import '@awesome.me/webawesome/dist/components/badge/badge.js'
import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/callout/callout.js'
import '@awesome.me/webawesome/dist/components/card/card.js'
import '@awesome.me/webawesome/dist/components/spinner/spinner.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

import CatalogIndexFixture from './fixtures/CatalogIndexFixture.svelte'

function catalog(title: string, overrides: Record<string, unknown> = {}) {
  return {
    id: `cat-${title}`,
    title,
    feed_url: `https://example.test/${title}/opds`,
    auth_kind: 'none',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

/** Resolved by the mocked command; a rejection drives the error branch. */
let respond: () => Promise<unknown>

beforeEach(() => {
  respond = async () => []
  invoke.mockReset()
  invoke.mockImplementation(async (command: string) => {
    if (command === 'opds_catalogs_list') return respond()
    throw new Error(`unexpected command: ${command}`)
  })
})

function empty(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>('p.empty')
}

function cards(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.catalog-link strong')].map(
    (n) => n.textContent?.trim() ?? '',
  )
}

test('invites the first subscription when none exist', async () => {
  const { container } = await render(CatalogIndexFixture)

  await expect.poll(() => empty(container)).not.toBeNull()

  expect(empty(container)?.textContent?.replace(/\s+/g, ' ').trim()).toContain(
    'No catalogs subscribed',
  )
  expect(cards(container)).toEqual([])
})

test('renders a card per subscribed catalog', async () => {
  respond = async () => [catalog('Standard Ebooks'), catalog('Gutenberg')]

  const { container } = await render(CatalogIndexFixture)

  await expect.poll(() => cards(container)).toEqual(['Standard Ebooks', 'Gutenberg'])
  expect(empty(container)).toBeNull()
  expect(container.querySelector('.catalog-url')?.textContent).toContain('example.test')
})

// auth_kind is only surfaced when it is not 'none', so an unauthenticated
// catalog must not render an empty badge.
test('badges a catalog that needs credentials, and only that one', async () => {
  respond = async () => [catalog('Open'), catalog('Private', { auth_kind: 'basic' })]

  const { container } = await render(CatalogIndexFixture)

  await expect.poll(() => cards(container)).toEqual(['Open', 'Private'])

  const badges = [...container.querySelectorAll('wa-badge')].map((n) => n.textContent?.trim())
  expect(badges).toEqual(['basic'])
})

test('surfaces a refused request as an error, not as an empty library', async () => {
  respond = async () => {
    throw { Network: 'catalog unreachable' }
  }

  const { container } = await render(CatalogIndexFixture)

  await expect.poll(() => container.querySelector('wa-callout')).not.toBeNull()

  expect(container.querySelector('wa-callout')?.textContent?.trim()).toBe('catalog unreachable')
  // The empty state is a sibling branch; a failed load must not claim there
  // are no catalogs.
  expect(empty(container)).toBeNull()
})

test('shows a spinner while the request is in flight', async () => {
  respond = () => new Promise(() => {})

  const { container } = await render(CatalogIndexFixture)

  await expect.poll(() => container.querySelector('wa-spinner')).not.toBeNull()
  expect(empty(container)).toBeNull()
  expect(container.querySelector('wa-callout')).toBeNull()
})
