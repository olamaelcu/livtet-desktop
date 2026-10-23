import { render } from 'svelte/server'
import { describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: vi.fn(async () => []),
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: vi.fn(() => ({ close: vi.fn() })),
}))

import AudioPage from '$routes/reader/audio/[editionId]/+page.svelte'
import PubPage from '$routes/reader/pub/[editionId]/+page.svelte'
import SsrShell from './SsrShell.svelte'

describe('audio reader page', () => {
  test('renders the reader shell without a navigation rail', () => {
    const { body } = render(SsrShell, { props: { page: AudioPage } })

    expect(body).toContain('data-route-shell')
    expect(body).not.toContain('aria-label="Primary"')
  })
})

describe('epub reader page', () => {
  test('renders the reader shell with a viewport region', () => {
    const { body } = render(SsrShell, { props: { page: PubPage } })

    expect(body).toContain('data-route-shell')
    expect(body).not.toContain('aria-label="Primary"')
  })
})
