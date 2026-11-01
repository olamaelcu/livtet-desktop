import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

// Keep the handlers so the test can make the daemon speak.
const { eventHandlers, listen } = vi.hoisted(() => {
  const eventHandlers = new Map<string, (event: { payload: unknown }) => void>()
  const listen = vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    eventHandlers.set(event, handler)
    return () => eventHandlers.delete(event)
  })
  return { eventHandlers, listen }
})
vi.mock('@tauri-apps/api/event', () => ({ listen }))

import PullRefreshFixture from './fixtures/PullRefreshFixture.svelte'

function emit(event: string, payload: unknown) {
  eventHandlers.get(event)?.({ payload })
}

beforeEach(() => {
  listen.mockClear()
  eventHandlers.clear()
})

test('a pull that added books refetches the library page', async () => {
  const { container } = await render(PullRefreshFixture)
  await expect.poll(() => container.querySelectorAll('.library li').length).toBe(1)
  expect(container.textContent).toContain('book 1')

  emit('sync://pulled', { device_id: 'peer', applied: 2, conflicts: 0 })

  // The page was served from cache with an infinite stale time, so a second
  // fetch is only possible if the pull dropped it.
  await expect.poll(() => container.textContent).toContain('book 2')
})

test('a request this device served is not this library changing', async () => {
  const { container } = await render(PullRefreshFixture)
  await expect.poll(() => container.querySelectorAll('.library li').length).toBe(1)

  emit('sync://completed', { path: '/sync/changes', status: 200 })

  // Serving a peer's pull fills their library; refetching here would be a
  // wasted round trip on every sync tick.
  await expect.poll(() => container.textContent).toContain('book 1')
  expect(container.textContent).not.toContain('book 2')
})
