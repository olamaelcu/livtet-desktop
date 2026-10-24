import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import PubPageFixture from './fixtures/PubPageFixture.svelte'

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

test('renders pub page shell', async () => {
  const { container } = await render(PubPageFixture)

  expect(container).toBeInTheDocument()
})