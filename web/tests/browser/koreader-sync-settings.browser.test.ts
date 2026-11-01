import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/callout/callout.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/switch/switch.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

import KoreaderSyncFixture from './fixtures/KoreaderSyncFixture.svelte'

type WaSwitch = HTMLElement & { checked: boolean; updateComplete?: Promise<unknown> }
type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

type Status = {
  settings: {
    enabled: boolean
    port: number
    lan: boolean
    username: string | null
    paired: boolean
    password: string | null
  }
  running: boolean
  url: string | null
  books: number
}

const OFF: Status = {
  settings: {
    enabled: false,
    port: 8081,
    lan: true,
    username: null,
    paired: false,
    password: null,
  },
  running: false,
  url: null,
  books: 3,
}

const ON: Status = {
  settings: {
    enabled: true,
    port: 8081,
    lan: true,
    username: 'livtet',
    paired: true,
    password: 'hunter2',
  },
  running: true,
  url: 'http://192.168.1.10:8081',
  books: 3,
}

let status: Status
let saveResult: Status

beforeEach(() => {
  status = OFF
  saveResult = ON
  invoke.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'koreader_sync_status':
        return status
      case 'koreader_sync_settings_save':
        return saveResult
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

function control<T extends Element>(container: HTMLElement, selector: string): T {
  const node = container.querySelector<T>(selector)
  expect(node, `the panel renders ${selector}`).not.toBeNull()
  return node as T
}

async function setSwitch(container: HTMLElement, selector: string, checked: boolean) {
  const toggle = control<WaSwitch>(container, selector)
  await toggle.updateComplete
  toggle.checked = checked
  toggle.dispatchEvent(new Event('change', { bubbles: true }))
}

async function setInput(container: HTMLElement, selector: string, value: string) {
  const input = control<WaInput>(container, selector)
  await input.updateComplete
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

async function clickSave(container: HTMLElement) {
  const button = control<WaButton>(container, '#koreader-sync-save')
  await button.updateComplete
  button.click()
}

test('shows the stored configuration and that nothing is listening', async () => {
  const { container } = await render(KoreaderSyncFixture)

  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  expect(control<WaSwitch>(container, '#koreader-sync-enabled').checked).toBe(false)
  expect(control<WaInput>(container, '#koreader-sync-port').value).toBe('8081')
  expect(container.textContent).toContain('Not listening.')
  expect(container.textContent).toContain('3 books mapped')
})

test('an unpaired server refuses to save without credentials', async () => {
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setSwitch(container, '#koreader-sync-enabled', true)
  await clickSave(container)

  await expect.poll(() => container.textContent).toContain('A username and password are required')
  expect(invoke.mock.calls.some(([command]) => command === 'koreader_sync_settings_save')).toBe(
    false,
  )
})

test('saving enabled credentials forwards the form and shows the URL', async () => {
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setSwitch(container, '#koreader-sync-enabled', true)
  await setInput(container, '#koreader-sync-username', 'livtet')
  await setInput(container, '#koreader-sync-password', 'hunter2')
  await clickSave(container)

  await expect
    .poll(
      () => invoke.mock.calls.find(([command]) => command === 'koreader_sync_settings_save')?.[1],
    )
    .toEqual({
      settings: {
        enabled: true,
        port: 8081,
        lan: true,
        username: 'livtet',
        password: 'hunter2',
      },
    })

  await expect.poll(() => container.textContent).toContain('http://192.168.1.10:8081')
  expect(toast.success).toHaveBeenCalledWith('KOReader sync listening')
})

test('prefills the remembered password from the keyring', async () => {
  status = ON
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  expect(control<WaInput>(container, '#koreader-sync-password').value).toBe('hunter2')
})

test('an untouched password field keeps the stored one', async () => {
  status = ON
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setInput(container, '#koreader-sync-port', '8082')
  await clickSave(container)

  await expect
    .poll(
      () => invoke.mock.calls.find(([command]) => command === 'koreader_sync_settings_save')?.[1],
    )
    .toEqual({
      settings: { enabled: true, port: 8082, lan: true, username: 'livtet', password: null },
    })
})

test('an out-of-range port blocks the save and explains why', async () => {
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setInput(container, '#koreader-sync-port', '70000')

  await expect
    .poll(() => container.querySelector('p.error')?.textContent)
    .toContain('Port must be between 1 and 65535')
  expect(control<WaButton>(container, '#koreader-sync-save').disabled).toBe(true)
  expect(invoke.mock.calls.some(([command]) => command === 'koreader_sync_settings_save')).toBe(
    false,
  )
})

test('a refused save restores the switch and reports the backend error', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'koreader_sync_status') return status
    throw { code: 'bind', message: 'bind 0.0.0.0:8081: address in use' }
  })
  const { container } = await render(KoreaderSyncFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setSwitch(container, '#koreader-sync-enabled', true)
  await setInput(container, '#koreader-sync-username', 'livtet')
  await setInput(container, '#koreader-sync-password', 'hunter2')
  await clickSave(container)

  await expect.poll(() => toast.error.mock.calls[0]?.[0]).toBe('bind 0.0.0.0:8081: address in use')
  expect(control<WaSwitch>(container, '#koreader-sync-enabled').checked).toBe(false)
})

test('a loopback-only server says an e-reader cannot reach it', async () => {
  status = {
    settings: {
      enabled: true,
      port: 8081,
      lan: false,
      username: 'livtet',
      paired: true,
      password: null,
    },
    running: true,
    url: 'http://127.0.0.1:8081',
    books: 1,
  }
  const { container } = await render(KoreaderSyncFixture)

  await expect.poll(() => container.textContent).toContain('Reachable from other devices')
  expect(container.textContent).toContain('Bound to this machine only.')
  expect(container.textContent).toContain('1 book mapped')
})
