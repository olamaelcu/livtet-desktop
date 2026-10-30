import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { afterEach, expect, test } from 'vitest'
import { render } from 'vitest-browser-svelte'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import QueryProvider from './fixtures/QueryProvider.svelte'

const STORAGE_KEY = 'livtet.layout.toggleIcons'

afterEach(() => localStorage.removeItem(STORAGE_KEY))

// The rail's collapsed state is a stored preference, not a prop: a mount must
// take it from localStorage (and default to the labels-visible rail when the
// preference is absent).
test('NavRail collapse state comes from localStorage on mount', async () => {
  localStorage.removeItem(STORAGE_KEY)
  const fresh = await render(
    NavRail,
    { items: NAV_ITEMS, currentPath: '/library' },
    { wrapper: QueryProvider },
  )
  expect(
    (fresh.container.querySelector('.rail') as HTMLElement).classList.contains('collapsed'),
  ).toBe(false)

  localStorage.setItem(STORAGE_KEY, 'true')
  const collapsed = await render(
    NavRail,
    { items: NAV_ITEMS, currentPath: '/catalog' },
    { wrapper: QueryProvider },
  )
  expect(
    (collapsed.container.querySelector('.rail') as HTMLElement).classList.contains('collapsed'),
  ).toBe(true)

  localStorage.setItem(STORAGE_KEY, 'false')
  const expanded = await render(
    NavRail,
    { items: NAV_ITEMS, currentPath: '/settings' },
    { wrapper: QueryProvider },
  )
  expect(
    (expanded.container.querySelector('.rail') as HTMLElement).classList.contains('collapsed'),
  ).toBe(false)
})
