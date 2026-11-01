/**
 * The settings route's tabs, and the `?tab=` query parameter that selects one.
 *
 * The parameter is the source of truth for which tab is on screen: a reload, a
 * link into the route, and the tab strip all read the same value, so the
 * address bar can never disagree with the visible panel.
 */

/** One settings tab: the name its panel carries, and the label the strip shows. */
export interface SettingsTab {
  /** `wa-tab`/`wa-tab-panel` name, and the value `?tab=` carries. */
  name: string
  /** Text the strip draws for it. */
  label: string
  /** Icon drawn before the label. */
  icon: string
  /** Only offered on macOS (desktop integrations CoreSpotlight owns). */
  macOnly?: boolean
}

/** The tabs, in the order the strip draws them. The first is the default. */
export const SETTINGS_TABS: readonly SettingsTab[] = [
  // Catalog sources and the built-in OPDS server are one tab: the panel holds a
  // nested tab group of its own (see the route), because those settings only
  // make sense read together.
  { name: 'opds', label: 'OPDS', icon: 'rss' },
  { name: 'sync', label: 'Sync', icon: 'arrows-rotate' },
  { name: 'account', label: 'Account', icon: 'user' },
  { name: 'hotkeys', label: 'Keyboard', icon: 'keyboard' },
  { name: 'desktop', label: 'Desktop', icon: 'desktop', macOnly: true },
  { name: 'mcp', label: 'MCP', icon: 'plug' },
]

/** The tabs this platform offers, in draw order. */
export function availableSettingsTabs(isMacOs: boolean): readonly SettingsTab[] {
  return SETTINGS_TABS.filter((tab) => !tab.macOnly || isMacOs)
}

/**
 * The tab a query string selects, or `undefined` when it names none of them.
 *
 * An unknown or absent value reports nothing rather than guessing, so the
 * caller can fall back to the default tab. Platform-hidden tabs are unknown.
 */
export function settingsTabFrom(
  search: string,
  tabs: readonly SettingsTab[] = SETTINGS_TABS,
): string | undefined {
  const wanted = new URLSearchParams(search).get('tab')
  return tabs.find((tab) => tab.name === wanted)?.name
}

/**
 * The settings URL that opens `tab`, with the rest of the query untouched.
 *
 * The default tab is carried by the bare path: `/settings` and
 * `/settings?tab=opds` are the same panel, and only one of them should be
 * reachable by clicking around.
 */
export function settingsTabUrl(url: URL, tab: string): URL {
  const next = new URL(url)
  if (tab === SETTINGS_TABS[0]?.name) next.searchParams.delete('tab')
  else next.searchParams.set('tab', tab)
  return next
}
