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

import OpdsServerFixture from './fixtures/OpdsServerFixture.svelte'

type WaSwitch = HTMLElement & { checked: boolean; updateComplete?: Promise<unknown> }
type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

type Status = {
  settings: { enabled: boolean; port: number }
  running: boolean
  url: string | null
}

const OFF: Status = { settings: { enabled: false, port: 8080 }, running: false, url: null }

const ON: Status = {
  settings: { enabled: true, port: 8080 },
  running: true,
  url: 'http://192.168.1.10:8080',
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
      case 'opds_server_status':
        return status
      case 'opds_server_settings_save':
        return saveResult
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

function control<T>(container: HTMLElement, selector: string): T {
  const node = container.querySelector<T>(selector)
  expect(node, `the panel renders ${selector}`).not.toBeNull()
  return node as T
}

/** Svelte assigns custom-element props as properties, so the DOM has to be
 * mutated the same way the control does it. */
async function setPort(container: HTMLElement, value: string) {
  const input = control<WaInput>(container, '.controls wa-input')
  await input.updateComplete
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

async function setEnabled(container: HTMLElement, checked: boolean) {
  const toggle = control<WaSwitch>(container, '.controls wa-switch')
  await toggle.updateComplete
  toggle.checked = checked
  toggle.dispatchEvent(new Event('change', { bubbles: true }))
}

async function clickSave(container: HTMLElement) {
  const button = control<WaButton>(container, '.controls wa-button')
  await button.updateComplete
  button.click()
}

test('shows the stored configuration and that nothing is listening', async () => {
  const { container } = await render(OpdsServerFixture)

  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  expect(control<WaSwitch>(container, '.controls wa-switch').checked).toBe(false)
  expect(control<WaInput>(container, '.controls wa-input').value).toBe('8080')
  expect(container.textContent).toContain('Not listening.')
})

test('warns that the catalog is reachable without a password', async () => {
  const { container } = await render(OpdsServerFixture)

  await expect.poll(() => container.textContent).toContain('no password')
  expect(container.textContent).toContain('every network interface')
})

test('saving an enabled server forwards the form and shows the URL it bound', async () => {
  const { container } = await render(OpdsServerFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setEnabled(container, true)
  await clickSave(container)

  await expect
    .poll(() => invoke.mock.calls.find(([command]) => command === 'opds_server_settings_save')?.[1])
    .toEqual({ settings: { enabled: true, port: 8080 } })

  await expect.poll(() => container.textContent).toContain('http://192.168.1.10:8080')
  expect(toast.success).toHaveBeenCalledWith('OPDS server listening')
})

test('an out-of-range port blocks the save and explains why', async () => {
  const { container } = await render(OpdsServerFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setEnabled(container, true)
  await setPort(container, '70000')

  await expect
    .poll(() => container.querySelector('p.error')?.textContent)
    .toContain('Port must be between 1 and 65535')
  expect(control<WaButton>(container, '.controls wa-button').disabled).toBe(true)
  expect(invoke.mock.calls.some(([command]) => command === 'opds_server_settings_save')).toBe(false)
})

test('a refused save restores the switch and reports the backend error', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'opds_server_status') return status
    throw { code: 'bind', message: 'bind 0.0.0.0:8080: address in use' }
  })
  const { container } = await render(OpdsServerFixture)
  await expect.poll(() => container.querySelector('.controls')).not.toBeNull()

  await setEnabled(container, true)
  await clickSave(container)

  await expect.poll(() => toast.error.mock.calls[0]?.[0]).toBe('bind 0.0.0.0:8080: address in use')
  expect(control<WaSwitch>(container, '.controls wa-switch').checked).toBe(false)
})
