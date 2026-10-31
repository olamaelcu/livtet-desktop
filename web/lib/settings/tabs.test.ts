import { describe, expect, it } from 'vitest'
import { SETTINGS_TABS, settingsTabFrom, settingsTabUrl } from './tabs'

describe('SETTINGS_TABS', () => {
  it('names every tab the route draws panels for, once each', () => {
    const names = SETTINGS_TABS.map((tab) => tab.name)

    expect(names).toEqual(['sync', 'catalogs', 'account', 'hotkeys', 'mcp', 'opds'])
    expect(new Set(names).size).toBe(names.length)
  })
})

describe('settingsTabFrom', () => {
  it('reads the tab the query string names', () => {
    expect(settingsTabFrom('?tab=mcp')).toBe('mcp')
    expect(settingsTabFrom('?tab=hotkeys&other=1')).toBe('hotkeys')
  })

  // An unknown tab is not a tab: the caller must fall back rather than show a
  // panel that does not exist.
  it('reports no tab for a value the route has none of', () => {
    expect(settingsTabFrom('')).toBeUndefined()
    expect(settingsTabFrom('?other=1')).toBeUndefined()
    expect(settingsTabFrom('?tab=nope')).toBeUndefined()
    expect(settingsTabFrom('?tab=MCP')).toBeUndefined()
    expect(settingsTabFrom('?tab=')).toBeUndefined()
  })
})

describe('settingsTabUrl', () => {
  const url = new URL('http://localhost/settings')

  it('writes the tab into the query and keeps the path', () => {
    const next = settingsTabUrl(url, 'opds')

    expect(next.pathname).toBe('/settings')
    expect(next.searchParams.get('tab')).toBe('opds')
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
    const next = settingsTabUrl(new URL('http://localhost/settings?tab=mcp'), 'sync')

    expect(next.search).toBe('')
  })
})
