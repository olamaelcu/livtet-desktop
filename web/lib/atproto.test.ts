import { beforeEach, describe, expect, test, vi } from 'vitest'
import {
  atprotoErrorMessage,
  atprotoGetProfile,
  atprotoLogout,
  atprotoStatus,
  beginAtprotoLogin,
  completeAtprotoLogin,
  listenToAtprotoEvents,
  setAtprotoAvatar,
  updateAtprotoProfile,
} from '$lib/atproto'

// `vi.mock` is hoisted above the imports, so the fakes must be too.
const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

const STATUS = {
  did: 'did:plc:aaaaaaaaaaaaaaaaaaaaaaaa',
  scope: 'atproto',
  expires_at: '2026-01-02T03:04:05.000Z',
}

describe('atproto command wrappers', () => {
  beforeEach(() => {
    invoke.mockReset()
    listen.mockReset()
  })

  test('beginAtprotoLogin passes the handle', async () => {
    invoke.mockResolvedValue({ url: 'https://pds.example/authorize' })

    await expect(beginAtprotoLogin('reader.example')).resolves.toEqual({
      url: 'https://pds.example/authorize',
    })
    expect(invoke).toHaveBeenCalledWith('atproto_begin_login', { handle: 'reader.example' })
  })

  test('completeAtprotoLogin passes the callback URL', async () => {
    invoke.mockResolvedValue(STATUS)

    await completeAtprotoLogin('net.olamaelcu.livtet:/oauth-redirect?code=abc')
    expect(invoke).toHaveBeenCalledWith('atproto_complete_login', {
      callbackUrl: 'net.olamaelcu.livtet:/oauth-redirect?code=abc',
    })
  })

  test('atprotoStatus targets the status command', async () => {
    invoke.mockResolvedValue(null)

    await expect(atprotoStatus()).resolves.toBeNull()
    expect(invoke).toHaveBeenCalledWith('atproto_status')
  })

  test('atprotoLogout targets the logout command', async () => {
    invoke.mockResolvedValue(null)

    await atprotoLogout()
    expect(invoke).toHaveBeenCalledWith('atproto_logout')
  })

  test('updateAtprotoProfile passes both text fields', async () => {
    invoke.mockResolvedValue({ did: STATUS.did })

    await updateAtprotoProfile({ displayName: 'Reader', description: 'Reads books' })
    expect(invoke).toHaveBeenCalledWith('atproto_update_profile', {
      displayName: 'Reader',
      description: 'Reads books',
    })
  })

  test('setAtprotoAvatar passes the file path', async () => {
    invoke.mockResolvedValue({ did: STATUS.did })

    await setAtprotoAvatar('/tmp/avatar.png')
    expect(invoke).toHaveBeenCalledWith('atproto_set_avatar', {
      avatarPath: '/tmp/avatar.png',
    })
  })

  test('atprotoGetProfile targets the profile command', async () => {
    invoke.mockResolvedValue({ did: STATUS.did })

    await atprotoGetProfile()
    expect(invoke).toHaveBeenCalledWith('atproto_get_profile')
  })
})

describe('atprotoErrorMessage', () => {
  test('returns a string error unchanged', () => {
    expect(atprotoErrorMessage('token exchange failed')).toBe('token exchange failed')
  })

  test('reads the message out of a struct error variant', () => {
    expect(atprotoErrorMessage({ Authorization: { message: 'account did not match' } })).toBe(
      'account did not match',
    )
  })

  test('falls back for unknown shapes', () => {
    expect(atprotoErrorMessage(null)).toBe('ATProto request failed')
    expect(atprotoErrorMessage(undefined)).toBe('ATProto request failed')
    expect(atprotoErrorMessage(7)).toBe('ATProto request failed')
    expect(atprotoErrorMessage({})).toBe('ATProto request failed')
  })
})

describe('listenToAtprotoEvents', () => {
  beforeEach(() => {
    listen.mockReset()
  })

  test('subscribes to the login and error events and forwards payloads', async () => {
    const handlers: Record<string, (event: { payload: unknown }) => void> = {}
    listen.mockImplementation(
      async (name: string, handler: (event: { payload: unknown }) => void) => {
        handlers[name] = handler
        return () => {}
      },
    )

    const onLogin = vi.fn()
    const onError = vi.fn()

    const unlisten = await listenToAtprotoEvents({ onLogin, onError })

    expect(listen).toHaveBeenCalledWith('atproto://login', expect.any(Function))
    expect(listen).toHaveBeenCalledWith('atproto://error', expect.any(Function))

    handlers['atproto://login']({ payload: STATUS })
    handlers['atproto://error']({ payload: 'sign-in failed' })

    expect(onLogin).toHaveBeenCalledWith(STATUS)
    expect(onError).toHaveBeenCalledWith('sign-in failed')
    expect(typeof unlisten).toBe('function')
  })

  test('the returned unlisten stops both subscriptions', async () => {
    const stops = [vi.fn(), vi.fn()]
    listen.mockImplementationOnce(async () => stops[0]).mockImplementationOnce(async () => stops[1])

    const unlisten = await listenToAtprotoEvents({ onLogin: vi.fn(), onError: vi.fn() })

    unlisten()
    expect(stops[0]).toHaveBeenCalledOnce()
    expect(stops[1]).toHaveBeenCalledOnce()
  })
})
