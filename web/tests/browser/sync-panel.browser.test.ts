import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/button-group/button-group.js'
import '@awesome.me/webawesome/dist/components/input/input.js'
import '@awesome.me/webawesome/dist/components/qr-code/qr-code.js'
import { beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

// SyncPanel subscribes to daemon events on mount; outside Tauri there is no
// event bridge, so the subscription is a no-op that unsubscribes cleanly.
const { listen } = vi.hoisted(() => ({ listen: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen }))

const { toast } = vi.hoisted(() => ({
  toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() },
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
  toast.info.mockReset()

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
      case 'sync_nearby_devices':
        return []
      case 'sync_device_name_get':
        return null
      case 'sync_firewall_hint':
        return null
      case 'sync_pairing_begin':
        return { node_id: 'node-1', expires_at: 'soon' }
      case 'sync_pairing_approve':
        return { device_id: 'device-abc', session_token: 'sess-1' }
      case 'sync_diagnostics':
        return {
          server_running: true,
          host: '0.0.0.0',
          port: 7777,
          loopback: { target: '127.0.0.1:7777', status: 401, error: null },
          lan: {
            target: '192.168.1.137:7777',
            status: null,
            error: 'error sending request for url (http://192.168.1.137:7777/sync/status)',
          },
          peers: [],
          inbound: [],
          findings: [
            '192.168.1.137:7777 never answered. A firewall, VLAN isolation, or client isolation on the access point drops these packets; outbound pulls are unaffected by all three.',
          ],
        }
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

function facts(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>('dl.facts')
}

/** Scoped to the status block: other sections carry their own empty-state
 * paragraphs with the same class. */
function unavailable(container: HTMLElement): HTMLElement | null {
  return container.querySelector<HTMLElement>(`${STATUS_SECTION} p.muted`)
}

function factsText(container: HTMLElement): string {
  return facts(container)?.textContent?.replace(/\s+/g, ' ').trim() ?? ''
}

function serverCalloutText(container: HTMLElement): string {
  return (
    container
      .querySelector(`${STATUS_SECTION} wa-callout`)
      ?.textContent?.replace(/\s+/g, ' ')
      .trim() ?? ''
  )
}

test('renders the daemon facts once health and status resolve', async () => {
  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => facts(container)).not.toBeNull()

  const text = factsText(container)
  expect(text).toContain('device-abc')
  expect(text).toContain('12')
  expect(text).toContain('34')
  expect(serverCalloutText(container)).toContain('127.0.0.1:7777')
  expect(serverCalloutText(container)).toContain('Sharing on')
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
  // The node id lives in a `wa-input` property, not text; the expiry line
  // proves the ticket block rendered.
  expect(container.textContent).toContain('Expires soon.')
  // Scanning the QR must yield the same value the input shows.
  const qr = container.querySelector('wa-qr-code') as HTMLElement & { value?: string }
  expect(qr).not.toBeNull()
  expect(qr.value).toBe('livtet://sync?v=2&nodeId=node-1')
  expect(invoke).toHaveBeenCalledWith('sync_pairing_begin', expect.anything())
})

test('approving a pending pairing toasts and calls approve', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_pairing_list') {
      return [
        {
          node_id: 'node-9',
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
          outbound: true,
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

  container.querySelector<HTMLElement>('.conflict wa-button')?.click()
  await expect.poll(() => toast.success).toHaveBeenCalledWith('Conflict resolved')
})

test('bulk resolves every unresolved conflict with one choice', async () => {
  const resolvedIds: number[] = []
  const resolutions: string[] = []
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
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
        {
          id: 8,
          entity_type: 'book',
          entity_id: 'ed-2',
          local_payload: 'c',
          remote_payload: 'd',
          resolved: false,
          resolution: null,
          merged_payload: null,
          detected_at: 'today',
        },
        {
          id: 9,
          entity_type: 'book',
          entity_id: 'ed-3',
          local_payload: 'e',
          remote_payload: 'f',
          resolved: true,
          resolution: 'local',
          merged_payload: null,
          detected_at: 'yesterday',
        },
      ]
    }
    if (command === 'sync_conflicts_resolve') {
      resolvedIds.push(args?.id as number)
      resolutions.push(args?.resolution as string)
      return true
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('book ed-1')

  const section = container.querySelector<HTMLElement>(
    'section[aria-labelledby="sync-conflicts-heading"]',
  )
  const select = section?.querySelector<HTMLSelectElement>('select')
  expect(select).toBeInstanceOf(HTMLSelectElement)
  if (!select) throw new Error('expected a bulk resolution select')
  select.value = 'remote'
  select.dispatchEvent(new Event('change', { bubbles: true }))

  panelButton(container, 'Resolve all (2)').click()
  await expect.poll(() => container.querySelector('button.danger')).not.toBeNull()
  container.querySelector<HTMLButtonElement>('button.danger')?.click()

  await expect.poll(() => toast.success).toHaveBeenCalledWith('Resolved 2 of 2 conflicts as remote')
  expect(resolvedIds).toEqual([7, 8])
  expect(resolutions).toEqual(['remote', 'remote'])
})

test('lists a discovered peer with a pictogram and its friendly name', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_nearby_devices') {
      return [
        {
          node_id: 'peer-node',
          device_id: 'peer-device',
          name: 'Amber Otter',
          device_type: 'ios',
          version: '0.1.0',
          hostname: 'livtet-peer.local',
          addresses: ['192.168.1.40'],
          port: 4321,
        },
      ]
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })

  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => container.textContent).toContain('Amber Otter')
  expect(container.textContent).toContain('192.168.1.40:4321')
  expect(container.textContent).toContain('📱')

  const pair = [...container.querySelectorAll('wa-button')].find(
    (button) => button.textContent?.trim() === 'Pair',
  )
  expect(pair).not.toBeUndefined()
  pair?.click()
  await expect
    .poll(() => invoke)
    .toHaveBeenCalledWith('sync_pair_claim', {
      listenOn: '192.168.1.40:4321',
      nodeId: 'peer-node',
      deviceName: 'Amber Otter',
    })
})

test('saves the chosen device name', async () => {
  invoke.mockImplementation(async (command: string, args?: { name?: string | null }) => {
    if (command === 'sync_device_name_get') return 'Old Name'
    if (command === 'sync_device_name_save') return args?.name ?? null
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })

  const { container } = await render(SyncPanelFixture)
  const input = container.querySelector<HTMLElement & { value: string }>('.device-name-input')
  expect(input).not.toBeNull()

  await expect.poll(() => input?.value).toBe('Old Name')

  if (!input) return
  input.value = "  Josh's MacBook  "
  input.dispatchEvent(new Event('input', { bubbles: true }))
  panelButton(container, 'Save').click()

  await expect.poll(() => toast.success).toHaveBeenCalledWith('Device name saved')
  expect(invoke).toHaveBeenCalledWith('sync_device_name_save', { name: "Josh's MacBook" })
})

test('shows the server as stopped while the daemon is still reachable', async () => {
  health = { ...(health as Record<string, unknown>), server_running: false }
  status = { ...(status as Record<string, unknown>), server_running: false }

  const { container } = await render(SyncPanelFixture)

  await expect.poll(() => facts(container)).not.toBeNull()
  expect(serverCalloutText(container)).toContain('Server stopped')
})

test('diagnosing reachability reports what each probe answered', async () => {
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => facts(container)).not.toBeNull()

  const diagnose = [...container.querySelectorAll<HTMLElement>('wa-button, button')].find(
    (button) => button.textContent?.trim() === 'Diagnose',
  )
  expect(diagnose).not.toBeUndefined()
  diagnose?.click()

  const reach = 'section[aria-labelledby="sync-reach-heading"]'
  await expect.poll(() => container.querySelector(`${reach} .findings`)).not.toBeNull()

  const findings = container.querySelector(`${reach} .findings`)?.textContent ?? ''
  expect(findings).toContain('firewall')
  expect(findings).not.toContain('pair again')

  // The probes are shown as they answered: a 401 is reachability, an error is not.
  await expect
    .poll(() => container.querySelector(`${reach} .probes`)?.textContent ?? '')
    .toContain('127.0.0.1:7777 answered 401')
  expect(container.querySelector(`${reach} .probes`)?.textContent).toContain(
    '192.168.1.137:7777: error sending request',
  )
})

// Pairing is directional. A device that paired *to* this desktop holds the
// token this side issued, so the daemon cannot pull from it; the listed flag
// says so, and the pull is never attempted rather than refused.
test('a device that paired to this one is never pulled from', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_devices_list') {
      return [
        {
          device_id: 'device-inbound',
          name: 'Laptop',
          listen_on: '0.0.0.0:7710',
          device_type_id: null,
          paired_at: 'yesterday',
          last_sync_at: null,
          outbound: false,
        },
      ]
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('Laptop')
  expect(container.textContent).toContain('One-way')

  panelButton(container, 'Check for books').click()

  await expect
    .poll(() => toast.info)
    .toHaveBeenCalledWith(
      'Nothing can be pulled from Laptop',
      expect.objectContaining({ description: expect.stringContaining('Re-pair') }),
    )
  expect(toast.error).not.toHaveBeenCalled()
  const pulled = invoke.mock.calls.filter(([command]) => command === 'sync_pull')
  expect(pulled).toHaveLength(0)
})

// The receive path itself: a device this desktop claimed had an offer waiting.
test('pulling a claimed device reports the offers it brought', async () => {
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_devices_list') {
      return [
        {
          device_id: 'device-claimed',
          name: 'Laptop',
          listen_on: '192.168.1.137:7710',
          device_type_id: null,
          paired_at: 'today',
          last_sync_at: null,
          outbound: true,
        },
      ]
    }
    if (command === 'sync_pull') {
      return { device_id: 'device-claimed', applied: 0, offered: 2, conflicts: 0 }
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('Laptop')
  expect(container.textContent).toContain('Two-way')

  panelButton(container, 'Check for books').click()

  await expect.poll(() => toast.success).toHaveBeenCalledWith('Received 2 offered books')
  expect(invoke).toHaveBeenCalledWith('sync_pull', { deviceId: 'device-claimed' })
  expect(toast.error).not.toHaveBeenCalled()
})

// A pull waits on the peer, so the button must say what it is waiting for
// instead of sitting on "Checking…".
test('a slow pull names the device it is waiting on', async () => {
  let release = () => {}
  const gate = new Promise<void>((resolve) => {
    release = resolve
  })
  invoke.mockImplementation(async (command: string) => {
    if (command === 'sync_devices_list') {
      return [
        {
          device_id: 'device-claimed',
          name: 'Laptop',
          listen_on: '192.168.1.137:7710',
          device_type_id: null,
          paired_at: 'today',
          last_sync_at: null,
          outbound: true,
        },
      ]
    }
    if (command === 'sync_pull') {
      await gate
      return { device_id: 'device-claimed', applied: 0, offered: 0, conflicts: 0 }
    }
    if (command === 'sync_health') return health
    if (command === 'sync_status') return status
    return []
  })
  const { container } = await render(SyncPanelFixture)
  await expect.poll(() => container.textContent).toContain('Laptop')

  panelButton(container, 'Check for books').click()
  await expect.poll(() => container.textContent).toContain('Pulling from Laptop')

  release()
  await expect.poll(() => toast.info).toHaveBeenCalledWith('Nothing new from your paired devices')
})
