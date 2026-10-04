import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { page } from 'vitest/browser'
import { render } from 'vitest-browser-svelte'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import DockFixture from './fixtures/DockFixture.svelte'
import ScrollFixture from './fixtures/ScrollFixture.svelte'
import ShellFixture from './fixtures/ShellFixture.svelte'

test('ScrollRegion is the only scrolling element and scroll events bubble', async () => {
  let bubbled = false
  const { container } = await render(ScrollFixture, { onscroll: () => (bubbled = true) })
  const region = container.querySelector<HTMLElement>('[data-scroll-region]')
  expect(region).not.toBeNull()
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  expect(getComputedStyle(region as HTMLElement).overflowY).toBe('auto')
  expect((region as HTMLElement).scrollHeight).toBeGreaterThan((region as HTMLElement).clientHeight)
  region?.dispatchEvent(new Event('scroll', { bubbles: true }))
  expect(bubbled).toBe(true)
})

test('RouteShell omits the nav column when no nav snippet is given', async () => {
  const { container } = await render(ShellFixture)
  const shell = container.querySelector('[data-route-shell]') as HTMLElement
  expect(shell).not.toBeNull()
  const tracks = getComputedStyle(shell).gridTemplateColumns.trim().split(/\s+/)
  expect(tracks).toHaveLength(1)
})

test('NavRail marks the active item with aria-current and keeps an accessible name when collapsed', async () => {
  const { container } = await render(NavRail, {
    items: NAV_ITEMS,
    currentPath: '/library',
    expanded: false,
  })
  const buttons = [...container.querySelectorAll<HTMLElement>('wa-button[href]')]
  expect(buttons).toHaveLength(NAV_ITEMS.length)
  const current = container.querySelector('[aria-current="page"]')
  expect(current?.getAttribute('href')).toBe('/library')
  for (const b of buttons) {
    const label = b.querySelector<HTMLElement>('.rail__label') as HTMLElement
    expect(label.textContent?.trim()).toBeTruthy()
    // Not visibly rendered: clipped to 1px, yet still in the accessibility tree.
    expect(getComputedStyle(label).display).not.toBe('none')
    expect(label.getBoundingClientRect().width).toBeLessThanOrEqual(1)
  }
  // The accessible name must survive collapse: resolve by role + name.
  for (const item of NAV_ITEMS) {
    await expect.element(page.getByRole('link', { name: item.label })).toBeInTheDocument()
  }
})

test('Dock positions against its Pane, not the viewport', async () => {
  const { container } = await render(DockFixture)
  const dock = container.querySelector('[data-dock]') as HTMLElement
  expect(dock).not.toBeNull()
  expect(getComputedStyle(dock).position).toBe('absolute')
  expect(dock.offsetParent).toBe(dock.parentElement)
  expect(getComputedStyle(dock.parentElement as HTMLElement).position).toBe('relative')
})
