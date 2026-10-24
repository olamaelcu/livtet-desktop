import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import AudioPageFixture from './fixtures/AudioPageFixture.svelte'

const { convertFileSrc, invoke } = vi.hoisted(() => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
  invoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({ convertFileSrc, invoke }))
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: vi.fn(() => ({ close: vi.fn() })) }))

beforeEach(() => {
  invoke.mockReset()
  invoke.mockImplementation(async () => null)
})

test('renders audio page shell', async () => {
  const { container } = await render(AudioPageFixture)

  expect(container).toBeInTheDocument()
  await expect.poll(() => container.querySelector('audio')).not.toBeNull()
  await expect.poll(() => container.querySelector('.reader')).not.toBeNull()
})