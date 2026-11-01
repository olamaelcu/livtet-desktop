import { beforeEach, describe, expect, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  getKoreaderSyncStatus,
  koreaderSyncErrorMessage,
  saveKoreaderSyncSettings,
  startKoreaderSync,
  stopKoreaderSync,
} from './koreader-sync'

beforeEach(() => {
  invoke.mockReset()
})

describe('koreaderSyncErrorMessage', () => {
  test('returns a plain string error as-is', () => {
    expect(koreaderSyncErrorMessage('boom')).toBe('boom')
  })

  test('reads the message off a KoreaderSyncError', () => {
    expect(koreaderSyncErrorMessage({ code: 'bind', message: 'address in use' })).toBe(
      'address in use',
    )
    expect(koreaderSyncErrorMessage({ code: 'unpaired', message: 'set a username' })).toBe(
      'set a username',
    )
  })

  test('falls back for unrecognized shapes', () => {
    expect(koreaderSyncErrorMessage(null)).toBe('KOReader sync request failed')
    expect(koreaderSyncErrorMessage({})).toBe('KOReader sync request failed')
    expect(koreaderSyncErrorMessage({ message: '' })).toBe('KOReader sync request failed')
    expect(koreaderSyncErrorMessage(42)).toBe('KOReader sync request failed')
  })
})

test('getKoreaderSyncStatus reads the current status', async () => {
  invoke.mockResolvedValue({
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
    books: 0,
  })

  const status = await getKoreaderSyncStatus()

  expect(status.running).toBe(false)
  expect(invoke).toHaveBeenCalledWith('koreader_sync_status')
})

test('saveKoreaderSyncSettings forwards the settings under the command name', async () => {
  const status = {
    settings: {
      enabled: true,
      port: 8081,
      lan: true,
      username: 'livtet',
      paired: true,
      password: null,
    },
    running: true,
    url: 'http://192.168.1.10:8081',
    books: 4,
  }
  invoke.mockResolvedValue(status)

  const saved = await saveKoreaderSyncSettings({
    enabled: true,
    port: 8081,
    lan: true,
    username: 'livtet',
    password: 'hunter2',
  })

  expect(saved).toBe(status)
  expect(invoke).toHaveBeenCalledWith('koreader_sync_settings_save', {
    settings: {
      enabled: true,
      port: 8081,
      lan: true,
      username: 'livtet',
      password: 'hunter2',
    },
  })
})

test('start and stop call their own commands', async () => {
  invoke.mockResolvedValue({})

  await startKoreaderSync()
  await stopKoreaderSync()

  expect(invoke).toHaveBeenNthCalledWith(1, 'koreader_sync_start')
  expect(invoke).toHaveBeenNthCalledWith(2, 'koreader_sync_stop')
})
