import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test } from 'vitest'
import { page } from 'vitest/browser'
import { render } from 'vitest-browser-svelte'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import DockAfterScrollFixture from './fixtures/DockAfterScrollFixture.svelte'
import DockFixture from './fixtures/DockFixture.svelte'
import ListLayoutFixture from './fixtures/ListLayoutFixture.svelte'
import NarrowRailFixture from './fixtures/NarrowRailFixture.svelte'
import ScrollFixture from './fixtures/ScrollFixture.svelte'
import ShellFixture from './fixtures/ShellFixture.svelte'

test('ScrollRegion is the only scrolling element and a real scroll is observable', async () => {
  const { container } = await render(ScrollFixture)
  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  expect(getComputedStyle(region).overflowY).toBe('auto')
  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)

  // No ancestor scrolls: the region is the only scroll container.
  for (let el = region.parentElement; el; el = el.parentElement) {
    expect(['auto', 'scroll']).not.toContain(getComputedStyle(el).overflowY)
  }

  // Drive a real scroll; native scroll events do not bubble, so observe in the
  // capture phase at the host.
  const host = container.querySelector('#host') as HTMLElement
  const captured = new Promise<EventTarget | null>((resolve) =>
    host.addEventListener('scroll', (e) => resolve(e.target), { capture: true, once: true }),
  )
  region.scrollTop = 50
  expect(await captured).toBe(region)
  expect(region.scrollTop).toBe(50)

  // The region works as an IntersectionObserver root (what pagination needs).
  const probe = document.createElement('div')
  region.firstElementChild?.append(probe)
  probe.style.cssText = 'position:absolute;top:900px;height:10px'
  ;(region.firstElementChild as HTMLElement).style.position = 'relative'
  const seen = await new Promise<boolean>((resolve) => {
    const io = new IntersectionObserver((entries) => resolve(entries[0].isIntersecting), {
      root: region,
    })
    io.observe(probe)
  })
  expect(seen).toBe(false)
  region.scrollTop = 800
  const seenAfter = await new Promise<boolean>((resolve) => {
    const io = new IntersectionObserver((entries) => resolve(entries[0].isIntersecting), {
      root: region,
    })
    io.observe(probe)
  })
  expect(seenAfter).toBe(true)
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
  // aria-current must be on the real link inside wa-button's shadow root.
  await expect
    .poll(() =>
      buttons.map((b) => b.shadowRoot?.querySelector('a')?.getAttribute('aria-current') ?? null),
    )
    .toEqual(NAV_ITEMS.map((i) => (i.href === '/library' ? 'page' : null)))
  await expect
    .element(page.getByRole('link', { name: 'Library', exact: true }))
    .toHaveAttribute('href', '/library')
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

test('NavRail collapses to icon width below 46rem even when expanded, keeping its names', async () => {
  const { container } = await render(NarrowRailFixture, { width: '400px' })
  const rail = container.querySelector('.rail') as HTMLElement
  await expect.poll(() => rail.getBoundingClientRect().width).toBeCloseTo(52, 0)
  for (const label of container.querySelectorAll<HTMLElement>('.rail__label')) {
    expect(label.getBoundingClientRect().width).toBeLessThanOrEqual(1)
  }
  for (const item of NAV_ITEMS) {
    await expect.element(page.getByRole('link', { name: item.label })).toBeInTheDocument()
  }
})

test('NavRail shows full width with labels when expanded in a wide container', async () => {
  const { container } = await render(NarrowRailFixture, { width: '900px' })
  const rail = container.querySelector('.rail') as HTMLElement
  await expect.poll(() => rail.getBoundingClientRect().width).toBeCloseTo(176, 0)
  const label = container.querySelector('.rail__label') as HTMLElement
  expect(label.getBoundingClientRect().width).toBeGreaterThan(1)
})

test('Pane still grows its ScrollRegion when a Dock follows it in source order', async () => {
  const { container } = await render(DockAfterScrollFixture)
  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(region.clientHeight).toBeLessThanOrEqual(200)
  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)
  region.scrollTop = 50
  expect(region.scrollTop).toBe(50)
})

test('the app frame never scrolls and each route has exactly one scroll region', async () => {
  const { container } = await render(ListLayoutFixture)
  const frame = container.firstElementChild as HTMLElement
  expect(getComputedStyle(frame).overflowY).toBe('hidden')
  expect(frame.scrollHeight).toBe(frame.clientHeight)
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)
  expect(container.querySelector('nav[aria-label="Primary"]')).not.toBeNull()
})
