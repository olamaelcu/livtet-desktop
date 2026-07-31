import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

describe('loadCustomProfile', () => {
  let getItem: ReturnType<typeof vi.fn>

  beforeEach(() => {
    getItem = vi.fn()
    vi.stubGlobal('window', {
      localStorage: { getItem },
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('returns empty object when window is undefined', async () => {
    vi.stubGlobal('window', undefined)
    const { loadCustomProfile } = await import('./storage')
    expect(loadCustomProfile()).toEqual({})
  })

  it('returns empty object when localStorage has no entry', async () => {
    getItem.mockReturnValue(null)
    const { loadCustomProfile } = await import('./storage')
    expect(loadCustomProfile()).toEqual({})
  })

  it('parses valid JSON', async () => {
    const profile = { 'palette.open': 'Mod+K' }
    getItem.mockReturnValue(JSON.stringify(profile))
    const { loadCustomProfile } = await import('./storage')
    expect(loadCustomProfile()).toEqual(profile)
  })

  it('returns empty object on JSON parse error', async () => {
    getItem.mockReturnValue('not-json')
    const { loadCustomProfile } = await import('./storage')
    expect(loadCustomProfile()).toEqual({})
  })

  it('returns empty object when parsed value is null', async () => {
    getItem.mockReturnValue('null')
    const { loadCustomProfile } = await import('./storage')
    expect(loadCustomProfile()).toEqual({})
  })
})
