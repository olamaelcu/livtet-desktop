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
      case 'sync_pairing_begin':
        return { token: 'tok-1', uri: 'livtet://pair/tok-1', expires_at: 'soon' }
      case 'sync_pairing_approve':
        return { device_id: 'device-abc', session_token: 'sess-1' }
      case 'sync_pairing_reject':
      case 'sync_devices_revoke':
      case 'sync_conflicts_resolve':
      case 'sync_server_start':
      case 'sync_server_stop':
        return true
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

function panelButton(container: HTMLElement, label: string): HTMLElement {
  const found = [...container.querySelectorAll<HTMLElement>('wa-button')].find((button) =>
    button.textContent?.includes(label),
  )
  expect(found, `a "${label}" button`).not.toBeUndefined()
  return found as HTMLElement
}

test('pairing a device shows the ticket and toasts', async () => {
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => facts(container)).not.toBeNull()

  panelButton(container, 'Pair a device').click()
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Pairing ticket created')
  // The URI lives in `wa-input`/`wa-qr-code` properties, not text; the expiry
  // line proves the ticket block rendered.
  expect(container.textContent).toContain('Expires soon.')
  expect(invoke).toHaveBeenCalledWith('sync_pairing_begin', expect.anything())
})

test('approving a pending pairing toasts and calls approve', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_pairing_list') {
      return [
        {
          token: 'tok-9',
          desktop_id: 'd',
          listen_on: null,
          status_id: null,
          device_name: 'Phone',
          device_type_id: 'mobile',
          device_id: null,
          created_at: 'now',
          expires_at: 'later',
        },
      ]
    }
    if (command === 'sync_pairing_approve') {
      return { device_id: 'device-abc', session_token: 'sess-1' }
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('Phone')

  panelButton(container, 'Approve').click()
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Device paired')
  expect(invoke).toHaveBeenCalledWith('sync_pairing_approve', expect.anything())
})

test('revoking a device toasts', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_devices_list') {
      return [
        {
          device_id: 'device-abc',
          name: 'Laptop',
          listen_on: null,
          device_type_id: null,
          paired_at: 'yesterday',
          last_sync_at: null,
        },
      ]
    }
    if (command === 'sync_devices_revoke') return true
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('Laptop')

  panelButton(container, 'Revoke').click()
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Device revoked')
})

test('resolving a conflict toasts', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_conflicts_list') {
      return [
        {
          id: 7,
          entity_type: 'book',
          entity_id: 'ed-1',
          local_payload: 'a',
          remote_payload: 'b',
          resolved: false,
          resolution: null,
          merged_payload: null,
          detected_at: 'today',
        },
      ]
    }
    if (command === 'sync_conflicts_resolve') return true
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('book ed-1')

  panelButton(container, 'Resolve').click()
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Conflict resolved')
})

test('shows the server as stopped while the daemon is still reachable', async () => {
  health = { ...(health as Record<string, unknown>), server_running: false }
  status = { ...(status as Record<string, unknown>), server_running: false }

  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => facts(container)).not.toBeNull()
  expect(factsText(container)).toContain('stopped')
})
