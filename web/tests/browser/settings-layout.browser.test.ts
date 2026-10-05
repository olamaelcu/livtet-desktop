import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/tab/tab.js'
import '@awesome.me/webawesome/dist/components/tab-group/tab-group.js'
import '@awesome.me/webawesome/dist/components/tab-panel/tab-panel.js'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import SettingsFixture from './fixtures/SettingsFixture.svelte'

async function mount() {
  const { container } = await render(SettingsFixture)
  const group = container.querySelector('wa-tab-group') as HTMLElement
  await customElements.whenDefined('wa-tab-group')
  // The first tab activates asynchronously; wait until its panel is laid out.
  await vi.waitFor(() =>
    expect(group.querySelector('wa-tab-panel[active]')?.clientHeight).toBeGreaterThan(0),
  )
  const part = (name: string) => group.shadowRoot?.querySelector(`[part~="${name}"]`) as HTMLElement
  return { container, group, part, pane: group.parentElement as HTMLElement }
}

const isScroller = (el: Element) => ['auto', 'scroll'].includes(getComputedStyle(el).overflowY)

test('wa-tab-group brings no scroller of its own: only the body part scrolls once the route says so', async () => {
  const { group, part } = await mount()
  expect(isScroller(group)).toBe(false)
  expect(isScroller(part('base'))).toBe(false)
  expect(isScroller(part('nav'))).toBe(false)
  expect(isScroller(group.querySelector('wa-tab-panel') as HTMLElement)).toBe(false)
  // Added by SettingsLayout via ::part(body); wa-tab-group's default is visible.
  expect(getComputedStyle(part('body')).overflowY).toBe('auto')
})

test('the tab body scrolls and the pane does not', async () => {
  const { container, group, part, pane } = await mount()
  const body = part('body')
  expect(getComputedStyle(body).overflowY).toBe('auto')
  expect(body.scrollHeight).toBeGreaterThan(body.clientHeight)
  expect(pane.scrollHeight).toBe(pane.clientHeight)

  const frame = container.firstElementChild as HTMLElement
  expect(getComputedStyle(frame).overflowY).toBe('hidden')
  expect(frame.scrollHeight).toBe(frame.clientHeight)

  // Exactly one region, and it is the tab group; nothing else scrolls.
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)
  expect(container.querySelector('[data-scroll-region]')).toBe(group)
  expect(container.querySelector('nav[aria-label="Primary"]')).not.toBeNull()
  for (const el of container.querySelectorAll('*')) expect(isScroller(el)).toBe(false)

  // A real scroll moves the panel content while the tab strip stays put.
  const stripTop = part('nav').getBoundingClientRect().top
  const content = group.querySelector('wa-tab-panel') as HTMLElement
  const contentTop = content.getBoundingClientRect().top
  body.scrollTop = 500
  await vi.waitFor(() => expect(body.scrollTop).toBeGreaterThan(0))
  expect(part('nav').getBoundingClientRect().top).toBe(stripTop)
  expect(content.getBoundingClientRect().top).toBeLessThan(contentTop)
})

test('switching tabs does not grow the pane', async () => {
  const { group, part, pane } = await mount()
  const before = pane.clientHeight
  ;(group as HTMLElement & { active: string }).active = 'catalogs'
  await vi.waitFor(() =>
    expect(group.querySelector('wa-tab-panel[name="catalogs"]')?.hasAttribute('active')).toBe(true),
  )
  expect(pane.clientHeight).toBe(before)
  expect(pane.scrollHeight).toBe(pane.clientHeight)
  expect(part('body').scrollHeight).toBeGreaterThan(5000)
})
