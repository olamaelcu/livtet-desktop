// The "Copy files into library" switch must reach importFiles as mode 'copy'
// (ADR-0022). The handler once listened for `wa-change`, an event WebAwesome's
// switch never emits, so the toggle was dead while every mocked test stayed
// green. Only a real <wa-switch> can show that, hence a browser test.
import '@awesome.me/webawesome/dist/components/drawer/drawer.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/switch/switch.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { importFiles, openDialog } = vi.hoisted(() => ({
  importFiles: vi.fn(),
  openDialog: vi.fn(),
}))
vi.mock('../../lib/library/import', async (original) => ({
  ...(await original<typeof import('../../lib/library/import')>()),
  importFiles,
  listenToImportEvents: vi.fn(async () => () => undefined),
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openDialog }))
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  getCurrentWebviewWindow: () => ({ onDragDropEvent: async () => () => undefined }),
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('svelte-sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
}))

import AddBookDrawerFixture from './fixtures/AddBookDrawerFixture.svelte'

type WaSwitch = HTMLElement & { checked: boolean; updateComplete?: Promise<unknown> }

beforeEach(() => {
  importFiles.mockReset()
  openDialog.mockReset()
  importFiles.mockResolvedValue({ files: [], imported: 1, duplicated: 0, failed: 0 })
  openDialog.mockResolvedValue(['/tmp/a.epub'])
})

async function chooseFiles(container: HTMLElement) {
  const buttons = [...container.querySelectorAll<HTMLElement>('wa-button')]
  const button = buttons.find((b) => b.textContent?.includes('Choose files'))
  expect(button).toBeTruthy()
  button?.click()
  await expect.poll(() => importFiles.mock.calls.length).toBe(1)
}

test('toggling "Copy files into library" imports in copy mode', async () => {
  const { container } = await render(AddBookDrawerFixture)
  const control = container.querySelector<WaSwitch>('wa-switch')
  expect(control).toBeTruthy()
  await control?.updateComplete

  // A real user click on the switch: WebAwesome flips `checked` and fires a native `change`.
  control?.click()
  await control?.updateComplete
  expect(control?.checked).toBe(true)

  await chooseFiles(container)
  expect(importFiles).toHaveBeenCalledWith(['/tmp/a.epub'], 'copy')
})

test('leaving the switch alone imports as a link', async () => {
  const { container } = await render(AddBookDrawerFixture)
  await chooseFiles(container)
  expect(importFiles).toHaveBeenCalledWith(['/tmp/a.epub'], 'link')
})
