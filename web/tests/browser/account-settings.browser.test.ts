import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/card/card.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const { listen } = vi.hoisted(() => ({ listen: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

const { toast } = vi.hoisted(() => ({ toast: { error: vi.fn(), success: vi.fn() } }))
vi.mock('svelte-sonner', () => ({ toast }))

import AccountSettingsFixture from './fixtures/AccountSettingsFixture.svelte'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

const DID = 'did:plc:aaaaaaaaaaaaaaaaaaaaaaaa'

let status: Record<string, unknown> | null
let profile: Record<string, unknown> | null

beforeEach(() => {
  status = null
  profile = null

  invoke.mockReset()
  listen.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  listen.mockResolvedValue(() => {})

  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
    switch (command) {
      case 'atproto_status':
        return status
      case 'atproto_get_profile':
        return profile
      case 'atproto_begin_login':
        return { url: 'https://pds.example/authorize?client_id=livtet' }
      case 'atproto_logout':
        return null
      default:
        throw new Error(`unexpected command: ${command} ${JSON.stringify(args)}`)
    }
  })
})

async function clickButton(container: HTMLElement, label: string) {
  const button = [...container.querySelectorAll<WaButton>('wa-button')].find(
    (node) => node.textContent?.trim() === label,
  )
  expect(button, `a ${label} button is present`).not.toBeUndefined()
  await button?.updateComplete
  button?.click()
}

/** Svelte assigns `label` as a property, so attribute selectors do not match. */
function handleInput(container: HTMLElement): WaInput | null {
  return container.querySelector<WaInput>('.sign-in wa-input')
}

test('signed out shows the handle form and a sign-in action', async () => {
  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.sign-in')).not.toBeNull()
  expect(handleInput(container)).not.toBeNull()
  const labels = [...container.querySelectorAll('wa-button')].map(
    (node) => node.textContent?.trim() ?? '',
  )
  expect(labels).toContain('Sign in')
  expect(labels).not.toContain('Sign out')
})

test('signing in submits the trimmed handle and shows the fallback URL', async () => {
  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => handleInput(container)).not.toBeNull()

  const input = handleInput(container)
  if (!input) throw new Error('missing handle input')
  await input.updateComplete
  input.value = '  reader.example  '
  input.dispatchEvent(new Event('input', { bubbles: true }))

  // The button starts disabled while the handle is empty; wait for Svelte to
  // re-render it before clicking, or the click is swallowed.
  await expect
    .poll(
      () =>
        [...container.querySelectorAll<WaButton>('wa-button')].find(
          (node) => node.textContent?.trim() === 'Sign in',
        )?.disabled,
    )
    .toBe(false)

  await clickButton(container, 'Sign in')

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'atproto_begin_login')).toBe(true)
  const call = invoke.mock.calls.find(([c]) => c === 'atproto_begin_login')
  expect(call?.[1]).toEqual({ handle: 'reader.example' })

  await expect
    .poll(() => container.querySelector('.login-url')?.textContent ?? '')
    .toContain('https://pds.example/authorize')
})

test('signed in shows the DID, scope and expiry with a sign-out action', async () => {
  status = {
    did: DID,
    scope: 'atproto repo:x',
    expires_at: '2026-01-02T03:04:05.000Z',
  }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.session-details')).not.toBeNull()
  const details = container.querySelector('.session-details')?.textContent ?? ''
  expect(details).toContain(DID)
  expect(details).toContain('atproto repo:x')
  expect(details).toContain('2026-01-02T03:04:05.000Z')

  await expect
    .poll(() => container.querySelector('.profile')?.textContent ?? '')
    .toContain('Reader')
  expect(
    [...container.querySelectorAll('wa-button')].map((node) => node.textContent?.trim() ?? ''),
  ).toContain('Sign out')
})

test('a null profile reports that no record exists yet', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = null

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.textContent ?? '').toContain('No profile record yet')
})

test('signing out calls the logout command and invalidates the session', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = null

  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => container.querySelector('.session')).not.toBeNull()

  await clickButton(container, 'Sign out')

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'atproto_logout')).toBe(true)
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Signed out of ATProto')
})

test('refreshing the profile calls the appview command', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = { did: DID, display_name: 'Reader' }

  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => container.querySelector('.session')).not.toBeNull()

  await clickButton(container, 'Refresh profile')

  await expect
    .poll(() => invoke.mock.calls.filter(([c]) => c === 'atproto_get_profile').length)
    .toBeGreaterThan(1)
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Profile refreshed')
})
