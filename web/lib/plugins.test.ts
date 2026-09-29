import { describe, expect, it } from 'vitest'
import { pluginErrorMessage } from './plugins'

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
