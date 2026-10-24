<script lang="ts">
import AudioPage from '$routes/reader/audio/[editionId]/+page.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(async () => [
    { type: 'audio', url: 'audio.mp3' },
    { type: 'cover', url: 'cover.jpg' },
  ]),
}))

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: vi.fn(),
}))

import { render } from 'vitest-browser-svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/webviewWindow', () => ({ getCurrentWindow: vi.fn(() => ({ close: vi.fn() })) }))

import { render } from 'vitest-browser-svelte'

test('renders audio reader shell', async () => {
  const { container } = await render(AudioPageFixture)
  expect(container).toContain('data-route-shell')
  expect(container).not.toContain('aria-label="Primary"')
  await expect.poll(() => container.textContent).toContain('Audio Reader')
})