import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/button-group/button-group.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/slider/slider.js'
import { expect, test, vi } from 'vitest'
import { page } from 'vitest/browser'
import { render } from 'vitest-browser-svelte'
import LibraryLayoutFixture from './fixtures/LibraryLayoutFixture.svelte'

test('the book grid scroll region is the IntersectionObserver root', async () => {
  const onvisible = vi.fn()
  const { container } = await render(LibraryLayoutFixture, { onvisible })
  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  expect(region.scrollHeight).toBeGreaterThan(region.clientHeight)

  // The sentinel is far below the bounded region: a viewport-rooted observer
  // would see it only by accident, a region-rooted one must not fire yet.
  await new Promise((r) => setTimeout(r, 200))
  expect(onvisible).not.toHaveBeenCalled()

  region.scrollTop = region.scrollHeight
  await vi.waitFor(() => expect(onvisible).toHaveBeenCalled())
})

test('an empty library still renders its empty state at non-zero height', async () => {
  const { container } = await render(LibraryLayoutFixture, { empty: true })
  const empty = container.querySelector<HTMLElement>('[data-testid="empty"]') as HTMLElement
  expect(empty.clientHeight).toBeGreaterThan(0)
})

test('the toolbar wraps rather than clipping at 569px', async () => {
  await page.viewport(569, 600)
  const { container } = await render(LibraryLayoutFixture, { chips: 8 })
  const toolbar = container.querySelector<HTMLElement>('[role="toolbar"]') as HTMLElement
  expect(toolbar.scrollWidth).toBeLessThanOrEqual(toolbar.clientWidth)
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(
    document.documentElement.clientWidth,
  )
})

test('rail, toolbar and dock controls are all tab-reachable and the dock does not trap focus', async () => {
  const { container } = await render(LibraryLayoutFixture, { selecting: true })
  const focusable = [
    ...container.querySelectorAll<HTMLElement>('wa-button, button, wa-slider, [tabindex="0"]'),
  ]
  const rail = focusable.filter((el) => el.closest('nav[aria-label="Primary"]'))
  const toolbar = focusable.filter((el) => el.closest('[role="toolbar"]'))
  const dock = focusable.filter((el) => el.closest('[data-dock]'))
  expect(rail.length).toBeGreaterThan(0)
  expect(toolbar.length).toBeGreaterThan(0)
  expect(dock).toHaveLength(2)
  // DOM order: rail, then toolbar, then dock; the dock is last, so Tab leaves it.
  const order = [rail[0], toolbar[0], dock[0]].map((el) => focusable.indexOf(el))
  expect(order).toEqual([...order].sort((a, b) => a - b))
  for (const button of dock) {
    expect(button.tabIndex).toBeGreaterThanOrEqual(0)
  }
  dock[0].focus()
  expect(document.activeElement).toBe(dock[0])
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
})
