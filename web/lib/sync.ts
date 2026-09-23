import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  commands,
  type SyncConflict,
  type SyncDevice,
  type SyncError,
  type SyncHealth,
  type SyncPairingApproved,
  type SyncPairingTicket,
  type SyncPendingPairing,
  type SyncRequestRecord,
  type SyncStatusInfo,
} from './bindings'

/** Envelope every specta sync command resolves to. */
type SyncResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: SyncError }

/** The daemon error variant carried inside a failed {@link SyncResult}. */
function isSyncError(value: unknown): value is SyncError {
  return (
    typeof value === 'object' &&
    value !== null &&
    ('Unavailable' in value || 'Rpc' in value || 'Protocol' in value)
  )
}

/**
 * Render a sync command failure as a message suitable for a toast. Accepts
 * `unknown` because a thrown value may be a daemon {@link SyncError}, a plain
 * `Error`, or anything else.
 */
export function syncErrorMessage(error: unknown): string {
  if (isSyncError(error)) {
    if ('Unavailable' in error && error.Unavailable) return error.Unavailable.message
    if ('Rpc' in error && error.Rpc) return error.Rpc.message
    if ('Protocol' in error && error.Protocol) return error.Protocol.message
  }
  if (error instanceof Error) return error.message
  return String(error)
}

/** Unwrap the `{ status, data, error }` envelope, throwing the daemon error. */
async function unwrap<T>(call: Promise<SyncResult<T>>): Promise<T> {
  const result = await call
  if (result.status === 'error') throw result.error
  return result.data
}

/** Liveness of the sync daemon process. */
export function syncHealth(): Promise<SyncHealth> {
  return unwrap<SyncHealth>(commands.syncHealth())
}

/** Aggregate daemon + server status. */
export function syncStatus(): Promise<SyncStatusInfo> {
  return unwrap<SyncStatusInfo>(commands.syncStatus())
}

/** The most recent requests the server served. */
export function syncRequestsRecent(limit = 50): Promise<SyncRequestRecord[]> {
  return unwrap<SyncRequestRecord[]>(commands.syncRequestsRecent(limit))
}

/** Mint a pairing ticket a remote device can redeem. */
export function syncPairingBegin(
  deviceType?: string,
  ttlSecs?: number,
): Promise<SyncPairingTicket> {
  return unwrap<SyncPairingTicket>(commands.syncPairingBegin(deviceType ?? null, ttlSecs ?? null))
}

/** Pairings awaiting approval or rejection. */
export function syncPairingList(): Promise<SyncPendingPairing[]> {
  return unwrap<SyncPendingPairing[]>(commands.syncPairingList())
}

/** Approve an outstanding pairing, minting its device + session. */
export function syncPairingApprove(token: string): Promise<SyncPairingApproved> {
  return unwrap<SyncPairingApproved>(commands.syncPairingApprove(token))
}

/** Reject an outstanding pairing. */
export function syncPairingReject(token: string): Promise<boolean> {
  return unwrap<boolean>(commands.syncPairingReject(token))
}

/** Devices currently paired with this desktop. */
export function syncDevicesList(): Promise<SyncDevice[]> {
  return unwrap<SyncDevice[]>(commands.syncDevicesList())
}

/** Revoke a paired device's access. */
export function syncDevicesRevoke(deviceId: string): Promise<boolean> {
  return unwrap<boolean>(commands.syncDevicesRevoke(deviceId))
}

/** Unresolved sync conflicts. */
export function syncConflictsList(): Promise<SyncConflict[]> {
  return unwrap<SyncConflict[]>(commands.syncConflictsList())
}

/**
 * Resolve a conflict. `resolution` is `local`, `remote`, or `merged`; a
 * `merged` resolution carries the replacement payload.
 */
export function syncConflictsResolve(
  id: number,
  resolution: string,
  mergedPayload?: string,
): Promise<boolean> {
  return unwrap<boolean>(commands.syncConflictsResolve(id, resolution, mergedPayload ?? null))
}

/** Start the embedded sync server. */
export function syncServerStart(): Promise<boolean> {
  return unwrap<boolean>(commands.syncServerStart())
}

/** Stop the embedded sync server. */
export function syncServerStop(): Promise<boolean> {
  return unwrap<boolean>(commands.syncServerStop())
}

/** The Tauri events the sync daemon re-emits. */
export type SyncEventName =
  | 'sync://completed'
  | 'sync://pairing-requested'
  | 'sync://request'
  | 'sync://server-started'
  | 'sync://server-stopped'

type SyncEventHandler = (payload: unknown) => void

/**
 * Subscribe to the sync daemon's events. Resolves to a function that removes
 * every listener it registered.
 */
export async function listenToSyncEvents(
  handlers: Partial<Record<SyncEventName, SyncEventHandler>>,
): Promise<UnlistenFn> {
  const unlisteners = await Promise.all(
    Object.entries(handlers).map(([event, handler]) =>
      listen(event, (incoming) => handler?.(incoming.payload)),
    ),
  )
  return () => {
    for (const unlisten of unlisteners) unlisten()
  }
}
