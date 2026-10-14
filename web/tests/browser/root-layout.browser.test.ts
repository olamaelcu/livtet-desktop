import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(async () => []),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))

// Navigation and search-focus are the two side effects runCommand triggers
// that have no observable result in the DOM.
const { goto } = vi.hoisted(() => ({ goto: vi.fn(async () => {}) }))
vi.mock('$app/navigation', () => ({ goto }))

const { focusSearchTarget } = vi.hoisted(() => ({ focusSearchTarget: vi.fn(async () => {}) }))
vi.mock('$lib/hotkeys/searchTarget.svelte', () => ({ focusSearchTarget }))

import { queryClient } from '$lib/query/client'
import RootLayoutFixture from './fixtures/RootLayoutFixture.svelte'

beforeEach(() => {
  goto.mockClear()
  focusSearchTarget.mockClear()
  invoke.mockClear()
})

function palette(_container: HTMLElement): HTMLElement | null {
  // wa-dialog may be portalled out of the render container.
  return document.querySelector<HTMLElement>('wa-dialog')
}

function paletteItems(_container: HTMLElement): HTMLButtonElement[] {
  return [...document.querySelectorAll<HTMLButtonElement>('.palette-item')]
}

/** The palette has no on-screen trigger; Mod+K is the only way in. */
function pressPaletteHotkey() {
  for (const target of [window, document, document.body]) {
    for (const mods of [{ ctrlKey: true }, { metaKey: true }]) {
      target.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'k', bubbles: true, cancelable: true, ...mods }),
      )
    }
  }
}

function runFromPalette(container: HTMLElement, label: string) {
  const item = paletteItems(container).find((n) => n.textContent?.includes(label))
  expect(item, `the palette offers "${label}"`).not.toBeUndefined()
  item?.click()
}

test('renders the routed page inside the app frame', async () => {
  const { container } = await render(RootLayoutFixture)

  expect(container.querySelector('[data-testid="page-content"]')?.textContent).toBe('routed page')
})

test('the command palette stays shut until it is asked for', async () => {
  const { container } = await render(RootLayoutFixture)

  expect(palette(container)).toBeNull()
})

test('the palette hotkey opens it', async () => {
  const { container } = await render(RootLayoutFixture)

  pressPaletteHotkey()

  await expect.poll(() => palette(container)).not.toBeNull()
})

test('running a navigation command routes and closes the palette', async () => {
  const { container } = await render(RootLayoutFixture)
  pressPaletteHotkey()
  await expect.poll(() => palette(container)).not.toBeNull()

  runFromPalette(container, 'Go to Settings')

  await expect.poll(() => goto).toHaveBeenCalledWith('/settings')
  await expect.poll(() => palette(container)).toBeNull()
})

test('the library command routes to the library, not to settings', async () => {
  const { container } = await render(RootLayoutFixture)
  pressPaletteHotkey()
  await expect.poll(() => palette(container)).not.toBeNull()

  runFromPalette(container, 'Go to Library')

  await expect.poll(() => goto).toHaveBeenCalledWith('/library')
})

test('the focus-search command delegates to the search target', async () => {
  const { container } = await render(RootLayoutFixture)
  pressPaletteHotkey()
  await expect.poll(() => palette(container)).not.toBeNull()

  runFromPalette(container, 'Focus search')

  await expect.poll(() => focusSearchTarget).toHaveBeenCalled()
  expect(goto).not.toHaveBeenCalled()
})

test('the refresh command invalidates cached queries', async () => {
  const invalidate = vi.spyOn(queryClient, 'invalidateQueries')
  const { container } = await render(RootLayoutFixture)
  pressPaletteHotkey()
  await expect.poll(() => palette(container)).not.toBeNull()

  runFromPalette(container, 'Refresh data')

  await expect.poll(() => invalidate).toHaveBeenCalled()
  expect(goto).not.toHaveBeenCalled()
  invalidate.mockRestore()
})
