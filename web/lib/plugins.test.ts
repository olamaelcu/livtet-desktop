import { beforeEach, describe, expect, it, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { pluginErrorMessage, setPluginEnabled } from './plugins'

beforeEach(() => {
  invoke.mockReset()
})

describe('pluginErrorMessage', () => {
  it('returns a plain string error as-is', () => {
    expect(pluginErrorMessage('boom')).toBe('boom')
  })

  it('unwraps a tagged PluginError variant', () => {
    expect(pluginErrorMessage({ Invalid: { message: 'bad manifest' } })).toBe('bad manifest')
    expect(pluginErrorMessage({ Unavailable: { message: 'no registry' } })).toBe('no registry')
    expect(pluginErrorMessage({ Host: { message: 'host crashed' } })).toBe('host crashed')
  })

  it('falls back for unrecognized shapes', () => {
    expect(pluginErrorMessage(null)).toBe('Plugin request failed')
    expect(pluginErrorMessage({})).toBe('Plugin request failed')
    expect(pluginErrorMessage(42)).toBe('Plugin request failed')
  })
})

test('setPluginEnabled forwards the name and enabled flag to the command', async () => {
  invoke.mockResolvedValue(null)

  await setPluginEnabled('txt-importer', false)
  expect(invoke).toHaveBeenCalledWith('set_plugin_enabled', {
    name: 'txt-importer',
    enabled: false,
  })

  await setPluginEnabled('txt-importer', true)
  expect(invoke).toHaveBeenLastCalledWith('set_plugin_enabled', {
    name: 'txt-importer',
    enabled: true,
  })

  // The caller decides what a refusal means, so the rejection is not swallowed.
  invoke.mockReset()
  invoke.mockRejectedValueOnce({ Invalid: { message: 'unsafe plugin name' } })
  await expect(setPluginEnabled('../escape', false)).rejects.toEqual({
    Invalid: { message: 'unsafe plugin name' },
  })
})
