import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import audioPage from '../../routes/reader/audio/[editionId]/+page.svelte?raw'
import pubPage from '../../routes/reader/pub/[editionId]/+page.svelte?raw'
import CatalogDetailFixture from './fixtures/CatalogDetailFixture.svelte'
import ReaderFixture from './fixtures/ReaderFixture.svelte'

test('the catalog detail route renders a nav rail and one scroll region', async () => {
  const { container } = await render(CatalogDetailFixture)
  const frame = container.firstElementChild as HTMLElement
  expect(getComputedStyle(frame).overflowY).toBe('hidden')
  expect(frame.scrollHeight).toBe(frame.clientHeight)
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)
  expect(getComputedStyle(region).overflowY).toBe('auto')
  const rail = container.querySelector('nav[aria-label="Primary"]')
  expect(rail).not.toBeNull()
  expect(rail?.querySelector('wa-button[href="/library"]')).not.toBeNull()
  expect(container.querySelector('main.browser')).toBeNull()
})

test('the reader layout renders no navigation rail', async () => {
  const { container } = await render(ReaderFixture)
  expect(container.querySelector('nav')).toBeNull()
  expect(container.querySelector('wa-button[href]')).toBeNull()
  const shell = container.querySelector('[data-route-shell]') as HTMLElement
  expect(getComputedStyle(shell).gridTemplateColumns.trim().split(/\s+/)).toHaveLength(1)
})

test('the reader frame never scrolls and the growing region is bounded by the window', async () => {
  const { container } = await render(ReaderFixture)
  const frame = container.firstElementChild as HTMLElement
  expect(getComputedStyle(frame).overflowY).toBe('hidden')
  expect(frame.scrollHeight).toBe(frame.clientHeight)
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  const viewport = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  // Bounded: the fixture's 3000px in-flow child does not stretch the viewport
  // past the frame. This guards Pane's `flex: 1 1 0` growth of
  // [data-scroll-region] — removing that attribute turns this red. It does NOT
  // guard Pane's `min-height: 0`: the fixture viewport's own `overflow: hidden`
  // already forces an automatic min-height of 0, so that rule stays covered only
  // by the real page, which sets min-height: 0 on .viewport itself.
  expect(viewport.clientHeight).toBeGreaterThan(0)
  expect(viewport.clientHeight).toBeLessThan(frame.clientHeight)
})

test('the reader routes carry no height: 100% chain', () => {
  // The only `height: 100%` allowed in the pub page is the Readium iframe's own,
  // which fills an absolutely positioned box and is not a chain. Strip that
  // block; nothing may remain.
  const withoutIframeRule = pubPage.replace(
    /:global\(iframe\.readium-navigator-iframe\)\s*\{[^}]*\}/,
    '',
  )
  expect(withoutIframeRule).not.toMatch(/height:\s*100%/)
  expect(pubPage).toMatch(/iframe\.readium-navigator-iframe\)\s*\{[^}]*height:\s*100%/)
  expect(audioPage).not.toMatch(/height:\s*100%/)
  expect(pubPage).toContain('<ReaderLayout>')
  expect(audioPage).toContain('<ReaderLayout>')
})
