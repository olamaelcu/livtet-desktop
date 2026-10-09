import '@awesome.me/webawesome/dist/components/badge/badge.js'
import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/callout/callout.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/tab/tab.js'
import '@awesome.me/webawesome/dist/components/tab-group/tab-group.js'
import '@awesome.me/webawesome/dist/components/tab-panel/tab-panel.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openDialog }))

const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

import AddPluginPanelFixture from './fixtures/AddPluginPanelFixture.svelte'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

function remote(id: string, overrides: Record<string, unknown> = {}) {
  return {
    id,
    name: `Plugin ${id}`,
    version: '1.2.3',
    description: `About ${id}`,
    signer: null,
    ...overrides,
  }
}

/** What discover_remote_plugins does when called. */
let discover: () => Promise<unknown>

beforeEach(() => {
  discover = async () => []
  invoke.mockReset()
  openDialog.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case 'discover_remote_plugins':
        return discover()
      case 'install_remote_plugin':
        return {
          name: `installed-${args?.id}`,
          version: '1.0.0',
          granted: [],
          signer: 'unsigned',
          enabled: true,
        }
      case 'add_plugin_from_path':
        return {
          name: 'from-folder',
          version: '1.0.0',
          granted: [],
          signer: 'unsigned',
          enabled: true,
        }
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

function urlInput(container: HTMLElement): WaInput {
  return container.querySelector('.row wa-input') as WaInput
}

function browseButton(container: HTMLElement): WaButton {
  return container.querySelector('.row wa-button') as WaButton
}

function folderButton(container: HTMLElement): WaButton {
  return container.querySelector('wa-tab-panel[name="folder"] wa-button') as WaButton
}

function resultNames(container: HTMLElement): string[] {
  return [...container.querySelectorAll('.result .name')].map((n) => n.textContent?.trim() ?? '')
}

async function typeUrl(container: HTMLElement, value: string) {
  const control = urlInput(container)
  await control.updateComplete
  control.value = value
  control.dispatchEvent(new Event('input', { bubbles: true }))
  return control
}

async function clickBrowse(container: HTMLElement) {
  await expect.poll(() => browseButton(container).disabled).toBe(false)
  browseButton(container).click()
}

test('lists what a repository offers', async () => {
  discover = async () => [remote('a'), remote('b')]
  const { container } = await render(AddPluginPanelFixture)

  await typeUrl(container, 'https://plugins.example.test')
  await clickBrowse(container)

  await expect.poll(() => resultNames(container)).toEqual(['Plugin a', 'Plugin b'])
  expect(invoke).toHaveBeenCalledWith('discover_remote_plugins', {
    registryUrl: 'https://plugins.example.test',
  })
})

test('trims the registry url before browsing', async () => {
  const { container } = await render(AddPluginPanelFixture)

  await typeUrl(container, '  https://plugins.example.test  ')
  await clickBrowse(container)

  await expect
    .poll(() => invoke.mock.calls.some(([c]) => c === 'discover_remote_plugins'))
    .toBe(true)
  expect(invoke).toHaveBeenCalledWith('discover_remote_plugins', {
    registryUrl: 'https://plugins.example.test',
  })
})

// An empty or whitespace-only url must not reach the backend.
test('does not browse a blank url', async () => {
  const { container } = await render(AddPluginPanelFixture)

  await typeUrl(container, '   ')
  await clickBrowse(container)

  expect(invoke).not.toHaveBeenCalledWith('discover_remote_plugins', expect.anything())
})

test('reports a failed browse and shows no results', async () => {
  discover = async () => {
    throw { Invalid: { message: 'no registry at that url' } }
  }
  const { container } = await render(AddPluginPanelFixture)

  await typeUrl(container, 'https://plugins.example.test')
  await clickBrowse(container)

  await expect.poll(() => container.querySelector('wa-callout')).not.toBeNull()
  expect(container.querySelector('wa-callout')?.textContent?.trim()).toContain(
    'no registry at that url',
  )
  expect(resultNames(container)).toEqual([])
})

test('a later successful browse clears the earlier error', async () => {
  discover = async () => {
    throw { Invalid: { message: 'no registry at that url' } }
  }
  const { container } = await render(AddPluginPanelFixture)
  await typeUrl(container, 'https://plugins.example.test')
  await clickBrowse(container)
  await expect.poll(() => container.querySelector('wa-callout')).not.toBeNull()

  discover = async () => [remote('a')]
  await clickBrowse(container)

  await expect.poll(() => resultNames(container)).toEqual(['Plugin a'])
  expect(container.querySelector('wa-callout')).toBeNull()
})

test('installing a listed plugin sends its id and the trimmed url', async () => {
  discover = async () => [remote('a'), remote('b')]
  const { container } = await render(AddPluginPanelFixture)
  await typeUrl(container, '  https://plugins.example.test  ')
  await clickBrowse(container)
  await expect.poll(() => resultNames(container)).toEqual(['Plugin a', 'Plugin b'])

  container.querySelectorAll<HTMLElement>('.result wa-button')[1].click()

  await expect.poll(() => toast.success).toHaveBeenCalled()
  expect(invoke).toHaveBeenCalledWith('install_remote_plugin', {
    registryUrl: 'https://plugins.example.test',
    id: 'b',
  })
})

test('choosing a folder installs from the selected path', async () => {
  openDialog.mockResolvedValue('/home/reader/plugins/txt')
  const { container } = await render(AddPluginPanelFixture)

  await folderButton(container).updateComplete
  folderButton(container).click()

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'add_plugin_from_path')).toBe(true)
  expect(invoke).toHaveBeenCalledWith('add_plugin_from_path', {
    source: '/home/reader/plugins/txt',
  })
})

// Cancelling the dialog yields null, which must not be installed.
test('cancelling the folder dialog installs nothing', async () => {
  openDialog.mockResolvedValue(null)
  const { container } = await render(AddPluginPanelFixture)

  await folderButton(container).updateComplete
  folderButton(container).click()

  await expect.poll(() => openDialog).toHaveBeenCalled()
  expect(invoke).not.toHaveBeenCalledWith('add_plugin_from_path', expect.anything())
})
