import '@awesome.me/webawesome/dist/components/badge/badge.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/switch/switch.js'
import '@awesome.me/webawesome/dist/components/tag/tag.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))
vi.mock('svelte-sonner', () => ({ toast }))

import PluginListFixture from './fixtures/PluginListFixture.svelte'

type WaSwitch = HTMLElement & { checked: boolean; updateComplete?: Promise<unknown> }

/** The backend's state, as the mocked commands see it. */
let installed: Record<string, boolean>

beforeEach(() => {
  installed = { 'txt-importer': true }
  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === 'list_plugins') {
      return Object.entries(installed).map(([name, enabled]) => ({
        name,
        version: '1.0.0',
        granted: ['log'],
        signer: 'unsigned',
        enabled,
      }))
    }
    if (command === 'set_plugin_enabled') {
      installed[args?.name as string] = args?.enabled as boolean
      return null
    }
    throw new Error(`unexpected command: ${command}`)
  })
})

function row(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>('li.card[data-enabled]')
}

function statusText(container: HTMLElement): string {
  return container.querySelector('wa-badge.status')?.textContent?.trim() ?? ''
}

async function waitForRow(container: HTMLElement): Promise<HTMLElement> {
  await expect.poll(() => row(container)).not.toBeNull()
  return row(container) as HTMLElement
}

async function toggleSwitch(container: HTMLElement): Promise<WaSwitch> {
  const control = container.querySelector<WaSwitch>('wa-switch[aria-label="Enable txt-importer"]')
  expect(control, 'the row carries a switch labelled with the plugin name').not.toBeNull()
  const element = control as WaSwitch
  // wa-switch forwards click() to an input in its shadow root, which only
  // exists after its first render.
  await element.updateComplete
  element.click()
  return element
}

test('toggling a plugin updates its switch and status badge', async () => {
  const { container } = await render(PluginListFixture)
  const card = await waitForRow(container)

  expect(card.getAttribute('data-enabled')).toBe('true')
  expect(statusText(container)).toBe('enabled')

  const control = await toggleSwitch(container)

  await expect.poll(() => statusText(container)).toBe('disabled')
  expect(control.checked).toBe(false)
  expect(card.getAttribute('data-enabled')).toBe('false')
  expect(card.classList.contains('disabled')).toBe(true)
  expect(invoke).toHaveBeenCalledWith('set_plugin_enabled', {
    name: 'txt-importer',
    enabled: false,
  })
  expect(installed['txt-importer']).toBe(false)
  expect(toast.error).not.toHaveBeenCalled()
})

test('a rejected toggle restores the previous switch and badge state', async () => {
  // Hold the command open so the optimistic state is observable before the
  // refusal lands: the row must flip, then flip back, not merely never move.
  let refuse: (error: unknown) => void = () => {}
  const listed = invoke.getMockImplementation()
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    if (command === 'set_plugin_enabled') {
      return new Promise((_resolve, reject) => {
        refuse = reject
      })
    }
    return listed?.(command, args)
  })

  const { container } = await render(PluginListFixture)
  const card = await waitForRow(container)
  expect(statusText(container)).toBe('enabled')

  const control = await toggleSwitch(container)
  await expect.poll(() => statusText(container)).toBe('disabled')
  expect(control.checked).toBe(false)

  refuse({ Invalid: { message: 'no plugin named "txt-importer" is installed' } })

  await expect.poll(() => statusText(container)).toBe('enabled')
  expect(control.checked).toBe(true)
  expect(card.getAttribute('data-enabled')).toBe('true')
  expect(card.classList.contains('disabled')).toBe(false)
  expect(toast.error).toHaveBeenCalledWith('no plugin named "txt-importer" is installed')
  // The backend never changed, so the list was never invalidated against it.
  expect(installed['txt-importer']).toBe(true)
})
