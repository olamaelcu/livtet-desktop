import { describe, expect, it } from 'vitest'
import { availableSettingsTabs, SETTINGS_TABS, settingsTabFrom, settingsTabUrl } from './tabs'

describe('SETTINGS_TABS', () => {
  it('names every tab the route draws panels for, once each', () => {
    const names = SETTINGS_TABS.map((tab) => tab.name)

    expect(names).toEqual(['opds', 'sync', 'account', 'hotkeys', 'desktop', 'mcp'])
    expect(new Set(names).size).toBe(names.length)
  })
})

describe('availableSettingsTabs', () => {
  it('offers the Desktop tab only on macOS', () => {
    expect(availableSettingsTabs(true).map((tab) => tab.name)).toContain('desktop')
    expect(availableSettingsTabs(false).map((tab) => tab.name)).not.toContain('desktop')
  })
})

describe('settingsTabFrom', () => {
  it('reads the tab the query string names', () => {
    expect(settingsTabFrom('?tab=mcp')).toBe('mcp')
    expect(settingsTabFrom('?tab=hotkeys&other=1')).toBe('hotkeys')
    expect(settingsTabFrom('?tab=sync')).toBe('sync')
  })

  // An unknown tab is not a tab: the caller must fall back rather than show a
  // panel that does not exist.
  it('reports no tab for a value the route has none of', () => {
    expect(settingsTabFrom('')).toBeUndefined()
    expect(settingsTabFrom('?other=1')).toBeUndefined()
    expect(settingsTabFrom('?tab=nope')).toBeUndefined()
    expect(settingsTabFrom('?tab=MCP')).toBeUndefined()
    expect(settingsTabFrom('?tab=')).toBeUndefined()
    // Catalogs moved inside the OPDS tab, so links to it land on the default tab.
    expect(settingsTabFrom('?tab=catalogs')).toBeUndefined()
    // A macOS-only tab is not a valid target when the platform hides it.
    expect(settingsTabFrom('?tab=desktop', availableSettingsTabs(false))).toBeUndefined()
    expect(settingsTabFrom('?tab=desktop', availableSettingsTabs(true))).toBe('desktop')
  })
})

describe('settingsTabUrl', () => {
  const url = new URL('http://localhost/settings')

  it('writes the tab into the query and keeps the path', () => {
    const next = settingsTabUrl(url, 'account')

    expect(next.pathname).toBe('/settings')
    expect(next.searchParams.get('tab')).toBe('account')
    // The caller's URL is untouched: the address bar is the only thing that moves.
    expect(url.search).toBe('')
  })

  it('keeps the rest of the query', () => {
    const next = settingsTabUrl(
      new URL('http://localhost/settings?tab=mcp&from=library'),
      'account',
    )

    expect(next.searchParams.get('from')).toBe('library')
    expect(next.searchParams.get('tab')).toBe('account')
  })

  // `/settings` is the default tab, so clicking back to it must leave no trace:
  // otherwise one panel would have two addresses.
  it('drops the parameter for the default tab', () => {
    const next = settingsTabUrl(new URL('http://localhost/settings?tab=mcp'), 'opds')

    expect(next.search).toBe('')
  })
})
