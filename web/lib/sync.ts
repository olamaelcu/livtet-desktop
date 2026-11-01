import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  commands,
  type SyncBookOffer,
  type SyncConflict,
  type SyncDevice,
  type SyncError,
  type SyncHealth,
  type SyncIncomingOffer,
  type SyncNearbyPeer,
  type SyncOfferCover,
  type SyncOfferDecision,
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
    ('Unavailable' in value || 'Rpc' in value || 'Protocol' in value || 'Storage' in value)
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
    if ('Storage' in error && error.Storage) return error.Storage.message
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

/** Begin a pairing: mint this device's pairing node identity for remote peers. */
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

/**
 * Peers currently advertising the sync service on the local network.
 *
 * The daemon listens on mDNS for a short window and answers with what it
 * heard, so this is cheap to poll but never instant.
 */
export function syncNearbyDevices(): Promise<SyncNearbyPeer[]> {
  return unwrap<SyncNearbyPeer[]>(commands.syncNearbyDevices())
}

/** The stored device name; `null` means a generated one is advertised. */
export function syncDeviceNameGet(): Promise<string | null> {
  return unwrap<string | null>(commands.syncDeviceNameGet())
}

/**
 * Save the name advertised to nearby devices. Passing `null` restores the
 * generated name; the daemon applies the change immediately.
 */
export function syncDeviceNameSave(name: string | null): Promise<string | null> {
  return unwrap<string | null>(commands.syncDeviceNameSave(name))
}

/** Whether the daemon accepts sync peers from the LAN. */
export function syncLanGet(): Promise<boolean> {
  return unwrap<boolean>(commands.syncLanGet())
}

/**
 * Persist the LAN preference.
 *
 * Applies at the next launch: the daemon binds its interface when the sidecar
 * is spawned. Data routes stay session-gated either way.
 */
export function syncLanSet(enabled: boolean): Promise<boolean> {
  return unwrap<boolean>(commands.syncLanSet(enabled))
}

/** Approve an outstanding pairing by its node identity, binding the device. */
export function syncPairingApprove(nodeId: string): Promise<SyncPairingApproved> {
  return unwrap<SyncPairingApproved>(commands.syncPairingApprove(nodeId))
}

/** Reject an outstanding pairing by its node identity. */
export function syncPairingReject(nodeId: string): Promise<boolean> {
  return unwrap<boolean>(commands.syncPairingReject(nodeId))
}

/** Devices currently paired with this desktop. */
export function syncDevicesList(): Promise<SyncDevice[]> {
  return unwrap<SyncDevice[]>(commands.syncDevicesList())
}

/**
 * Books paired devices have offered, waiting on the reader's decision.
 *
 * A pull records an offer instead of taking it, so nothing here is on disk
 * yet: accepting is what downloads the file and writes the catalog rows.
 */
export function syncOffersList(): Promise<SyncIncomingOffer[]> {
  return unwrap<SyncIncomingOffer[]>(commands.syncOffersList())
}

/**
 * Answer an offer. `delivered` says whether the offering device was reachable
 * to hear the answer; the decision stands either way.
 */
export function syncOffersDecide(offerId: string, accepted: boolean): Promise<SyncOfferDecision> {
  return unwrap<SyncOfferDecision>(commands.syncOffersDecide(offerId, accepted))
}

/** An offer's cover as a data URL, or `null` when the sender has none. */
export function syncOfferCover(offerId: string): Promise<SyncOfferCover | null> {
  return unwrap<SyncOfferCover | null>(commands.syncOfferCover(offerId))
}

/**
 * Offer an edition's file to one paired device.
 *
 * The other device asks its reader first: accepting is what pulls the file,
 * cover, and catalog record; declining ends it there. Nothing is transferred
 * by this call, and `sync://offer-decided` reports the answer.
 */
export function syncSendBook(editionId: string, deviceId: string): Promise<SyncBookOffer> {
  return unwrap<SyncBookOffer>(commands.syncSendBook(editionId, deviceId))
}

/** File a pairing claim with a discovered peer; resolves before it approves. */
export async function syncPairClaim(
  listenOn: string,
  nodeId: string,
  deviceName: string | null,
): Promise<void> {
  await unwrap<null>(commands.syncPairClaim(listenOn, nodeId, deviceName))
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
  | 'sync://pulled'
  | 'sync://pairing-requested'
  | 'sync://request'
  | 'sync://server-started'
  | 'sync://server-stopped'
  | 'sync://paired'
  | 'sync://offer-received'
  | 'sync://offer-decided'

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
