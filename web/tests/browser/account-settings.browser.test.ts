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

const { openDialog } = vi.hoisted(() => ({ openDialog: vi.fn() }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: openDialog }))

import AccountSettingsFixture from './fixtures/AccountSettingsFixture.svelte'

type WaInput = HTMLElement & { value: string; updateComplete?: Promise<unknown> }
type WaButton = HTMLElement & { disabled: boolean; updateComplete?: Promise<unknown> }

const DID = 'did:plc:aaaaaaaaaaaaaaaaaaaaaaaa'

let status: Record<string, unknown> | null
let profile: Record<string, unknown> | null
/** What the refresh command reports, and what the status query sees afterwards. */
let refreshed: Record<string, unknown> | null

beforeEach(() => {
  status = null
  profile = null
  refreshed = null

  invoke.mockReset()
  listen.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  listen.mockResolvedValue(() => {})
  openDialog.mockReset()
  openDialog.mockResolvedValue('/tmp/avatar.png')

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
      case 'atproto_complete_login':
        return { did: DID, scope: 'atproto', expires_at: null }
      case 'atproto_update_profile':
        return { did: DID, display_name: 'New name', description: 'New bio' }
      case 'atproto_set_avatar':
        return { did: DID, avatar_url: 'https://pds.example/blob?cid=new' }
      case 'atproto_refresh_session':
        return refreshed
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
    // Relative to the moment the test runs, so the bracketed reading is stable.
    expires_at: new Date(Date.now() + 42 * 60_000).toISOString(),
  }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.session-details')).not.toBeNull()
  const details = container.querySelector('.session-details')?.textContent ?? ''
  expect(details).toContain(DID)
  expect(
    [...container.querySelectorAll('.session-details ul code')].map((n) => n.textContent),
  ).toEqual(['atproto', 'repo:x'])

  // The raw instant stays available as the machine-readable value; what a reader
  // sees is their own clock and, beside it, how long that leaves.
  const time = container.querySelector<HTMLTimeElement>('.session-details time')
  expect(time?.getAttribute('datetime')).toBe(status.expires_at)
  expect(time?.textContent).toContain('(in 42 minutes)')
  expect(time?.querySelector('.relative')?.textContent).toBe('(in 42 minutes)')
  expect(time?.textContent ?? '').not.toContain('T')

  await expect
    .poll(() => container.querySelector('.profile-card .display-name')?.textContent ?? '')
    .toContain('Reader')
  expect(container.querySelector('.profile-card .handle')?.textContent?.trim()).toBe(
    '@reader.example',
  )
  expect(
    [...container.querySelectorAll('wa-button')].map((node) => node.textContent?.trim() ?? ''),
  ).toContain('Sign out')
})

test('an expired session offers to spend the refresh token, and follows the result', async () => {
  const expiredAt = new Date(Date.now() - 5 * 60_000).toISOString()
  const renewedAt = new Date(Date.now() + 60 * 60_000).toISOString()
  status = { did: DID, scope: 'atproto', expires_at: expiredAt }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }
  // The Rust side stores the refreshed session, so the status query sees it too.
  refreshed = { did: DID, scope: 'atproto', expires_at: renewedAt }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.expired')).not.toBeNull()

  status = refreshed
  await clickButton(container, 'Refresh session')

  await expect.poll(() => toast.success.mock.calls).toEqual([['Access token refreshed']])
  expect(invoke).toHaveBeenCalledWith('atproto_refresh_session')
  // The row goes away once the panel is showing a token with life left.
  await expect.poll(() => container.querySelector('.expired')).toBeNull()
  await expect.poll(() => container.querySelector('time')?.getAttribute('datetime')).toBe(renewedAt)
})

// The button is only about an expiry that has passed; a token with time left
// spends no refresh token just because the panel was opened.
test('a session that is still valid offers no refresh', async () => {
  status = {
    did: DID,
    scope: 'atproto',
    expires_at: new Date(Date.now() + 30 * 60_000).toISOString(),
  }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.session-details')).not.toBeNull()
  expect(container.querySelector('.expired')).toBeNull()
})

// A refresh that rotates nothing means the old token was still accepted. The
// stored timestamp cannot move on its own, so the panel stops calling it expired:
// telling a reader their session is dead sends them to sign in for nothing.
test('a refresh that changes nothing stops calling the token expired', async () => {
  const expiredAt = new Date(Date.now() - 5 * 60_000).toISOString()
  status = { did: DID, scope: 'atproto', expires_at: expiredAt }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }
  refreshed = { did: DID, scope: 'atproto', expires_at: expiredAt }

  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => container.querySelector('.expired')).not.toBeNull()

  await clickButton(container, 'Refresh session')

  await expect.poll(() => toast.success.mock.calls).toEqual([['The access token is still valid']])
  // The status query refetches the same expiry; it must not come back as expired.
  await expect.poll(() => container.querySelector('.expired')).toBeNull()
  await expect
    .poll(() => invoke.mock.calls.filter(([command]) => command === 'atproto_status'))
    .toHaveLength(2)
})

// A timestamp the platform cannot parse must not surface as "Invalid Date" once
// the panel has decided to humanise it: the stored value is the honest fallback.
test('an unparseable expiry is shown as stored', async () => {
  status = { did: DID, scope: 'atproto', expires_at: 'whenever' }
  profile = { did: DID, display_name: 'Reader', handle: 'reader.example' }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('.session-details')).not.toBeNull()
  expect(container.querySelector('.session-details time')).toBeNull()
  expect(container.querySelector('.session-details')?.textContent ?? '').toContain('whenever')
})

test('a null profile reports that no record exists yet', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = null

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.textContent ?? '').toContain('No profile record yet')
})

test('manually pasting the callback URL completes the exchange', async () => {
  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => container.querySelector('.manual wa-input')).not.toBeNull()

  const input = container.querySelector('.manual wa-input') as WaInput
  await input.updateComplete
  input.value = 'net.olamaelcu.livtet:/oauth-redirect?code=abc&state=xyz'
  input.dispatchEvent(new Event('input', { bubbles: true }))

  await expect
    .poll(
      () =>
        [...container.querySelectorAll<WaButton>('wa-button')].find(
          (node) => node.textContent?.trim() === 'Complete sign-in',
        )?.disabled,
    )
    .toBe(false)

  await clickButton(container, 'Complete sign-in')

  await expect
    .poll(() => invoke.mock.calls.some(([c]) => c === 'atproto_complete_login'))
    .toBe(true)
  const call = invoke.mock.calls.find(([c]) => c === 'atproto_complete_login')
  expect(call?.[1]).toEqual({
    callbackUrl: 'net.olamaelcu.livtet:/oauth-redirect?code=abc&state=xyz',
  })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Signed in to ATProto')
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

test('saving the profile writes it to the PDS', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = { did: DID, display_name: 'Old name', description: 'Old bio' }

  const { container } = await render(AccountSettingsFixture)
  await expect.poll(() => container.querySelector('.editor-trigger wa-button')).not.toBeNull()
  await clickButton(container, 'Edit profile')
  await expect.poll(() => container.querySelector('.profile-editor')).not.toBeNull()

  const inputs = [...container.querySelectorAll<WaInput>('.profile-editor wa-input')]
  expect(inputs).toHaveLength(2)
  await inputs[0].updateComplete
  await inputs[1].updateComplete
  inputs[0].value = 'New name'
  inputs[0].dispatchEvent(new Event('input', { bubbles: true }))
  inputs[1].value = 'New bio'
  inputs[1].dispatchEvent(new Event('input', { bubbles: true }))

  await expect
    .poll(
      () =>
        [...container.querySelectorAll<WaButton>('wa-button')].find(
          (node) => node.textContent?.trim() === 'Save to PDS',
        )?.disabled,
    )
    .toBe(false)
  await clickButton(container, 'Save to PDS')

  await expect
    .poll(() => invoke.mock.calls.some(([c]) => c === 'atproto_update_profile'))
    .toBe(true)
  const call = invoke.mock.calls.find(([c]) => c === 'atproto_update_profile')
  expect(call?.[1]).toEqual({ displayName: 'New name', description: 'New bio' })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Profile saved to your PDS')
})

test('shows the avatar and uploads a new one', async () => {
  status = { did: DID, scope: 'atproto', expires_at: null }
  profile = {
    did: DID,
    display_name: 'Reader',
    avatar_url: `https://livtet.olamaelcu.net/user/profileImage/${DID}`,
  }

  const { container } = await render(AccountSettingsFixture)

  await expect.poll(() => container.querySelector('img.avatar')).not.toBeNull()
  expect(container.querySelector('img.avatar')?.getAttribute('src')).toBe(
    `https://livtet.olamaelcu.net/user/profileImage/${DID}`,
  )

  await clickButton(container, 'Edit profile')
  await clickButton(container, 'Change avatar')

  await expect.poll(() => invoke.mock.calls.some(([c]) => c === 'atproto_set_avatar')).toBe(true)
  const call = invoke.mock.calls.find(([c]) => c === 'atproto_set_avatar')
  expect(call?.[1]).toEqual({ avatarPath: '/tmp/avatar.png' })
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Avatar updated')
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
