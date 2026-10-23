import { render } from 'svelte/server'
import { describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: vi.fn(async () => []),
}))

import RootPage from '$routes/+page.svelte'
import PluginsPage from '$routes/plugins/+page.svelte'
import SettingsPage from '$routes/settings/+page.svelte'
import SsrShell from './SsrShell.svelte'

describe('root page', () => {
  test('sets the title and shows the loading callout', () => {
    const { body, head } = render(SsrShell, { props: { page: RootPage } })

    expect(head).toContain('<title>livtet</title>')
    expect(body).toContain('Preparing your experience')
  })
})

describe('plugins page', () => {
  test('renders the title, intro, and both sections', () => {
    const { body } = render(SsrShell, { props: { page: PluginsPage } })

    expect(body).toContain('Plugins')
    expect(body).toContain('out-of-process')
    expect(body).toContain('Add a plugin')
    expect(body).toContain('Installed')
  })
})

describe('settings page', () => {
  test('renders the settings heading with all three tab panels', () => {
    const { body } = render(SsrShell, { props: { page: SettingsPage } })

    expect(body).toContain('Settings')
    expect(body).toContain('name="sync"')
    expect(body).toContain('name="catalogs"')
    expect(body).toContain('name="hotkeys"')
  })
})
