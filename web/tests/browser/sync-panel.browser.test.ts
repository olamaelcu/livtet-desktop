import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/button-group/button-group.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

// SyncPanel subscribes to daemon events on mount; outside Tauri there is no
// event bridge, so the subscription is a no-op that unsubscribes cleanly.
const { listen } = vi.hoisted(() => ({ listen: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn() },
}))
vi.mock('svelte-sonner', () => ({ toast }))

import SyncPanelFixture from './fixtures/SyncPanelFixture.svelte'

/** What the mocked daemon reports. */
let health: Record<string, unknown> | null
let status: Record<string, unknown> | null

beforeEach(() => {
  invoke.mockReset()
  listen.mockReset()
  toast.error.mockReset()
  toast.success.mockReset()
  listen.mockImplementation(async () => () => {})

  health = {
    status: 'ok',
    version: '0.1.0',
    pid: 4242,
    uptime_secs: 90,
    server_running: true,
    port: 7777,
  }
  status = {
    device_id: 'device-abc',
    server_running: true,
    host: '127.0.0.1',
    port: 7777,
    latest_version: 12,
    paired_device_count: 2,
    pending_pairing_count: 1,
    requests_served: 34,
    last_request_at: '2026-10-08T12:00:00Z',
  }

  invoke.mockImplementation(async (command: string) => {
    switch (command) {
      case 'sync_health':
        // A non-Error rejection is what the generated bindings turn into a
        // { status: 'error' } envelope; an Error would be rethrown instead.
        if (health === null) throw { Unavailable: { message: 'daemon not running' } }
        return health
      case 'sync_status':
        if (status === null) throw { Unavailable: { message: 'daemon not running' } }
        return status
      case 'sync_pairing_list':
      case 'sync_devices_list':
      case 'sync_conflicts_list':
      case 'sync_requests_recent':
        return []
      default:
        throw new Error(`unexpected command: ${command}`)
    }
  })
})

const STATUS_SECTION = 'section[aria-labelledby="sync-status-heading"]'

function facts(container: HTMLElement): HTMLTableElement | null {
  return container.querySelector<HTMLTableElement>('table.facts')
}

/** Scoped to the status block: other sections carry their own empty-state
 * paragraphs with the same class. */
function unavailable(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>(`${STATUS_SECTION} p.muted`)
}

function factsText(container: HTMLElement): string {
  return facts(container)?.textContent?.replace(/\s+/g, ' ').trim() ?? ''
}

test('renders the daemon facts once health and status resolve', async () => {
  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => facts(container)).not.toBeNull()

  const text = factsText(container)
  expect(text).toContain('device-abc')
  expect(text).toContain('127.0.0.1:7777')
  expect(text).toContain('running')
  expect(text).toContain('12')
  expect(text).toContain('34')
  expect(unavailable(container)).toBeNull()
  expect(toast.error).not.toHaveBeenCalled()
})

test('reports the daemon as unavailable when both queries are refused', async () => {
  health = null
  status = null

  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => unavailable(container)).not.toBeNull()

  expect(unavailable(container)?.textContent?.trim()).toBe('Sync daemon is unavailable.')
  // The facts table is the other arm of the same branch, so it must be absent
  // rather than merely empty.
  expect(facts(container)).toBeNull()
})

test('shows the server as stopped while the daemon is still reachable', async () => {
  health = { ...(health as Record<string, unknown>), server_running: false }
  status = { ...(status as Record<string, unknown>), server_running: false }

  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => facts(container)).not.toBeNull()
  expect(factsText(container)).toContain('stopped')
})
