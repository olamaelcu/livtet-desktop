import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/tab/tab.js'
import '@awesome.me/webawesome/dist/components/tab-group/tab-group.js'
import '@awesome.me/webawesome/dist/components/tab-panel/tab-panel.js'
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import { page } from '$app/state'
import SettingsFixture from './fixtures/SettingsFixture.svelte'
// The route keeps `?tab=` in step with the tab strip through `replaceState`.
// There is no router under test, so the stub records the URLs it was handed.
// Imported by path, not through `$app/navigation`: `svelte-check` resolves that
// specifier to SvelteKit's own types, which know nothing of the stub's recorder.
import { replacedUrls } from './stubs/app-navigation'

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
  ;(group as HTMLElement & { active: string }).active = 'mcp'
  await vi.waitFor(() =>
    expect(group.querySelector('wa-tab-panel[name="mcp"]')?.hasAttribute('active')).toBe(true),
  )
  expect(pane.clientHeight).toBe(before)
  expect(pane.scrollHeight).toBe(pane.clientHeight)
  expect(part('body').scrollHeight).toBeGreaterThan(5000)
})

describe('the tab the query parameter selects', () => {
  beforeEach(() => {
    replacedUrls.length = 0
    page.url.search = ''
  })

  /** The `?tab=` value the route last wrote, or `undefined` if it wrote nothing. */
  function writtenTab(): string | null | undefined {
    return replacedUrls.at(-1)?.searchParams.get('tab')
  }

  test('opens the tab the address bar asks for', async () => {
    page.url.search = '?tab=mcp'

    const { group } = await mount()

    await vi.waitFor(() =>
      expect(group.querySelector('wa-tab-panel[name="mcp"]')?.hasAttribute('active')).toBe(true),
    )
    expect(group.getAttribute('active')).toBe('mcp')
  })

  // A hand-written or stale link must land somewhere real rather than blank.
  test('falls back to the first tab when the value names none', async () => {
    page.url.search = '?tab=nonsense'

    const { group } = await mount()

    expect(group.getAttribute('active')).toBe('opds')
    expect(group.querySelector('wa-tab-panel[name="opds"]')?.hasAttribute('active')).toBe(true)
  })

  // After mount, the `active` attribute is the route's only lever: the panel it
  // names has to become visible, or a URL written by the tab strip would never
  // reach the screen.
  test('shows the panel named by an active attribute set after mount', async () => {
    const { group } = await mount()

    group.setAttribute('active', 'account')

    await vi.waitFor(() =>
      expect(group.querySelector('wa-tab-panel[name="account"]')?.hasAttribute('active')).toBe(
        true,
      ),
    )
  })

  test('writes the tab a reader switches to into the query', async () => {
    const { group } = await mount()
    expect(writtenTab()).toBeUndefined()

    group.querySelector<HTMLElement>('wa-tab[panel="hotkeys"]')?.click()

    await vi.waitFor(() => expect(writtenTab()).toBe('hotkeys'))
    // The panel switches in place: only the query moves (`settingsTabUrl` keeps
    // the path, which its own unit test pins).
    expect(replacedUrls.at(-1)?.pathname).toBe(page.url.pathname)
  })

  // `/settings` is the default tab, so returning to it leaves no parameter:
  // one panel, one address.
  test('drops the parameter when the reader returns to the first tab', async () => {
    page.url.search = '?tab=mcp'
    const { group } = await mount()

    group.querySelector<HTMLElement>('wa-tab[panel="opds"]')?.click()

    await vi.waitFor(() => expect(replacedUrls).not.toHaveLength(0))
    expect(writtenTab()).toBeNull()
  })
})
