import { beforeEach, describe, expect, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { getMcpStatus, mcpErrorMessage, saveMcpSettings } from './mcp'

beforeEach(() => {
  invoke.mockReset()
})

describe('mcpErrorMessage', () => {
  test('returns a plain string error as-is', () => {
    expect(mcpErrorMessage('boom')).toBe('boom')
  })

  test('reads the message off an McpSettingsError', () => {
    expect(
      mcpErrorMessage({ code: 'invalid-port', message: 'port must be between 1 and 65535' }),
    ).toBe('port must be between 1 and 65535')
    expect(mcpErrorMessage({ code: 'bind', message: 'address in use' })).toBe('address in use')
  })

  test('falls back for unrecognized shapes', () => {
    expect(mcpErrorMessage(null)).toBe('MCP settings request failed')
    expect(mcpErrorMessage({})).toBe('MCP settings request failed')
    expect(mcpErrorMessage({ message: '' })).toBe('MCP settings request failed')
    expect(mcpErrorMessage(42)).toBe('MCP settings request failed')
  })
})

test('saveMcpSettings forwards the settings under the command name', async () => {
  const status = {
    settings: { enabled: true, port: 7777 },
    running: true,
    url: 'http://127.0.0.1:7777/mcp',
    env_port: null,
  }
  invoke.mockResolvedValue(status)

  const saved = await saveMcpSettings({ enabled: true, port: 7777 })

  expect(saved).toBe(status)
  expect(invoke).toHaveBeenCalledWith('mcp_settings_save', {
    settings: { enabled: true, port: 7777 },
  })
})

test('getMcpStatus reads the current status', async () => {
  invoke.mockResolvedValue({
    settings: { enabled: false, port: 7777 },
    running: false,
    url: null,
    env_port: null,
  })

  const status = await getMcpStatus()

  expect(status.running).toBe(false)
  expect(invoke).toHaveBeenCalledWith('mcp_settings_get')
})
