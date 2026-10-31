import { beforeEach, describe, expect, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { getOpdsServerStatus, opdsServerErrorMessage, saveOpdsServerSettings } from './opds-server'

beforeEach(() => {
  invoke.mockReset()
})

describe('opdsServerErrorMessage', () => {
  test('returns a plain string error as-is', () => {
    expect(opdsServerErrorMessage('boom')).toBe('boom')
  })

  test('reads the message off an OpdsServerError', () => {
    expect(opdsServerErrorMessage({ code: 'bind', message: 'address in use' })).toBe(
      'address in use',
    )
    expect(
      opdsServerErrorMessage({ code: 'index', message: 'search index is not available' }),
    ).toBe('search index is not available')
  })

  test('falls back for unrecognized shapes', () => {
    expect(opdsServerErrorMessage(null)).toBe('OPDS server settings request failed')
    expect(opdsServerErrorMessage({})).toBe('OPDS server settings request failed')
    expect(opdsServerErrorMessage({ message: '' })).toBe('OPDS server settings request failed')
    expect(opdsServerErrorMessage(42)).toBe('OPDS server settings request failed')
  })
})

test('getOpdsServerStatus reads the current status', async () => {
  invoke.mockResolvedValue({ settings: { enabled: false, port: 8080 }, running: false, url: null })

  const status = await getOpdsServerStatus()

  expect(status.running).toBe(false)
  expect(invoke).toHaveBeenCalledWith('opds_server_status')
})

test('saveOpdsServerSettings forwards the settings under the command name', async () => {
  const status = {
    settings: { enabled: true, port: 8080 },
    running: true,
    url: 'http://192.168.1.10:8080',
  }
  invoke.mockResolvedValue(status)

  const saved = await saveOpdsServerSettings({ enabled: true, port: 8080 })

  expect(saved).toBe(status)
  expect(invoke).toHaveBeenCalledWith('opds_server_settings_save', {
    settings: { enabled: true, port: 8080 },
  })
})
