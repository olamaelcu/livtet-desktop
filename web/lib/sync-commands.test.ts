import { beforeEach, expect, test, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import {
  syncConflictsList,
  syncConflictsResolve,
  syncDevicesList,
  syncDevicesRevoke,
  syncErrorMessage,
  syncHealth,
  syncPairingApprove,
  syncPairingBegin,
  syncPairingList,
  syncPairingReject,
  syncRequestsRecent,
  syncServerStart,
  syncServerStop,
  syncStatus,
} from './sync'

beforeEach(() => {
  invoke.mockReset()
  // The generated bindings wrap raw results into `{ status, data }`
  // envelopes themselves; mocks return raw payloads.
  invoke.mockImplementation(async () => 'PAYLOAD')
})

test('queries forward to their Tauri commands', async () => {
  await syncHealth()
  await syncStatus()
  await syncPairingList()
  await syncDevicesList()
  await syncConflictsList()
  await syncRequestsRecent(50)

  expect(invoke).toHaveBeenCalledWith('sync_health')
  expect(invoke).toHaveBeenCalledWith('sync_status')
  expect(invoke).toHaveBeenCalledWith('sync_pairing_list')
  expect(invoke).toHaveBeenCalledWith('sync_devices_list')
  expect(invoke).toHaveBeenCalledWith('sync_conflicts_list')
  expect(invoke).toHaveBeenCalledWith('sync_requests_recent', { limit: 50 })
})

test('mutations forward arguments', async () => {
  await syncPairingBegin('mobile', 300)
  await syncPairingApprove('node-1')
  await syncPairingReject('node-1')
  await syncDevicesRevoke('dev-1')
  await syncConflictsResolve(7, 'local')
  await syncConflictsResolve(8, 'merged', '{}')
  await syncServerStart()
  await syncServerStop()

  expect(invoke).toHaveBeenCalledWith('sync_pairing_begin', { deviceType: 'mobile', ttlSecs: 300 })
  // Pairing is keyed by the peer's node identity, not a ticket token.
  expect(invoke).toHaveBeenCalledWith('sync_pairing_approve', { nodeId: 'node-1' })
  expect(invoke).toHaveBeenCalledWith('sync_pairing_reject', { nodeId: 'node-1' })
  expect(invoke).toHaveBeenCalledWith('sync_devices_revoke', { deviceId: 'dev-1' })
  expect(invoke).toHaveBeenCalledWith('sync_conflicts_resolve', {
    id: 7,
    resolution: 'local',
    mergedPayload: null,
  })
  expect(invoke).toHaveBeenCalledWith('sync_conflicts_resolve', {
    id: 8,
    resolution: 'merged',
    mergedPayload: '{}',
  })
  expect(invoke).toHaveBeenCalledWith('sync_server_start')
  expect(invoke).toHaveBeenCalledWith('sync_server_stop')
})

test('daemon errors surface their message', () => {
  expect(syncErrorMessage({ Unavailable: { message: 'down' } })).toBe('down')
  expect(syncErrorMessage(new Error('boom'))).toBe('boom')
  expect(syncErrorMessage('plain')).toBe('plain')
})

test('daemon errors throw the daemon error', async () => {
  invoke.mockRejectedValueOnce({ Unavailable: { message: 'down' } })

  await expect(syncHealth()).rejects.toEqual({ Unavailable: { message: 'down' } })
})
