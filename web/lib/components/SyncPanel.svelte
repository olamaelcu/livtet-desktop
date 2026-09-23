<script lang="ts">
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import type {
  SyncConflict,
  SyncDevice,
  SyncHealth,
  SyncPairingTicket,
  SyncPendingPairing,
  SyncRequestRecord,
  SyncStatusInfo,
} from '../bindings'
import {
  listenToSyncEvents,
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
} from '../sync'
import SyncButton from './SyncButton.svelte'

const POLL_MS = 5000
const REQUEST_LIMIT = 50

let health = $state<SyncHealth | null>(null)
let status = $state<SyncStatusInfo | null>(null)
let pending = $state<SyncPendingPairing[]>([])
let devices = $state<SyncDevice[]>([])
let conflicts = $state<SyncConflict[]>([])
let requests = $state<SyncRequestRecord[]>([])
let ticket = $state<SyncPairingTicket | null>(null)
let busy = $state(false)
let resolutions = $state<Record<number, string | undefined>>({})
let mergedPayloads = $state<Record<number, string | undefined>>({})

const running = $derived(status?.server_running ?? health?.server_running ?? false)

/** Run `load`, returning its value or `undefined`, optionally toasting errors. */
async function attempt<T>(load: () => Promise<T>, report: boolean): Promise<T | undefined> {
  try {
    return await load()
  } catch (error) {
    if (report) toast.error(syncErrorMessage(error))
    return undefined
  }
}

async function refreshHealth(report = false) {
  health = (await attempt(syncHealth, report)) ?? health
}

async function refreshStatus(report = false) {
  status = (await attempt(syncStatus, report)) ?? status
}

async function refreshPending(report = false) {
  pending = (await attempt(syncPairingList, report)) ?? pending
}

async function refreshDevices(report = false) {
  devices = (await attempt(syncDevicesList, report)) ?? devices
}

async function refreshConflicts(report = false) {
  conflicts = (await attempt(syncConflictsList, report)) ?? conflicts
}

async function refreshRequests(report = false) {
  requests = (await attempt(() => syncRequestsRecent(REQUEST_LIMIT), report)) ?? requests
}

async function refreshAll() {
  if (busy) return
  busy = true
  await Promise.all([
    refreshHealth(true),
    refreshStatus(true),
    refreshPending(true),
    refreshDevices(true),
    refreshConflicts(true),
    refreshRequests(true),
  ])
  busy = false
}

/** Run a mutating action, toasting success or the daemon error. */
async function act(message: string, action: () => Promise<unknown>) {
  if (busy) return
  busy = true
  try {
    await action()
    toast.success(message)
  } catch (error) {
    toast.error(syncErrorMessage(error))
  } finally {
    busy = false
  }
}

function startServer() {
  return act('Sync server started', async () => {
    await syncServerStart()
    await refreshStatus()
    await refreshHealth()
  })
}

function stopServer() {
  return act('Sync server stopped', async () => {
    await syncServerStop()
    await refreshStatus()
    await refreshHealth()
  })
}

function beginPairing() {
  return act('Pairing ticket created', async () => {
    ticket = await syncPairingBegin()
  })
}

function approvePairing(token: string) {
  return act('Device paired', async () => {
    await syncPairingApprove(token)
    await refreshPending()
    await refreshDevices()
    await refreshStatus()
  })
}

function rejectPairing(token: string) {
  return act('Pairing rejected', async () => {
    await syncPairingReject(token)
    await refreshPending()
    await refreshStatus()
  })
}

function revokeDevice(deviceId: string) {
  return act('Device revoked', async () => {
    await syncDevicesRevoke(deviceId)
    await refreshDevices()
    await refreshStatus()
  })
}

function resolveConflict(conflict: SyncConflict) {
  const resolution = resolutions[conflict.id] ?? 'local'
  const merged = resolution === 'merged' ? (mergedPayloads[conflict.id] ?? '') : undefined
  return act('Conflict resolved', async () => {
    await syncConflictsResolve(conflict.id, resolution, merged)
    await refreshConflicts()
  })
}

async function copy(label: string, value: string) {
  try {
    await navigator.clipboard.writeText(value)
    toast.success(`${label} copied`)
  } catch {
    toast.error(`Could not copy ${label}`)
  }
}

onMount(() => {
  let unlisten: (() => void) | undefined
  let disposed = false

  listenToSyncEvents({
    'sync://pairing-requested': () => {
      refreshPending()
      refreshStatus()
    },
    'sync://request': () => {
      refreshRequests()
      refreshStatus()
    },
    'sync://completed': () => {
      refreshStatus()
      refreshConflicts()
    },
    'sync://server-started': () => {
      refreshHealth()
      refreshStatus()
    },
    'sync://server-stopped': () => {
      refreshHealth()
      refreshStatus()
    },
  }).then((stop) => {
    if (disposed) stop()
    else unlisten = stop
  })

  refreshAll()
  const interval = setInterval(() => {
    refreshHealth()
    refreshStatus()
    refreshPending()
    refreshDevices()
    refreshRequests()
  }, POLL_MS)

  return () => {
    disposed = true
    unlisten?.()
    clearInterval(interval)
  }
})
</script>

<div class="sync-panel">
  <section class="block" aria-labelledby="sync-status-heading">
    <div class="block-header">
      <h3 id="sync-status-heading">Status</h3>
      <SyncButton onclick={refreshAll} disabled={busy}>Refresh</SyncButton>
    </div>
    {#if health || status}
      <ul class="facts">
        <li>Daemon: {health ? `${health.status} v${health.version}` : 'unknown'}</li>
        <li>Device: {status?.device_id ?? 'unknown'}</li>
        <li>Address: {status ? `${status.host}:${status.port}` : 'unknown'}</li>
        <li>Server: {running ? 'running' : 'stopped'}</li>
        <li>Latest version: {status?.latest_version ?? 0}</li>
        <li>Paired devices: {status?.paired_device_count ?? 0}</li>
        <li>Pending pairings: {status?.pending_pairing_count ?? 0}</li>
        <li>Requests served: {status?.requests_served ?? 0}</li>
        <li>Last request: {status?.last_request_at ?? 'never'}</li>
      </ul>
    {:else}
      <p class="muted">Sync daemon is unavailable.</p>
    {/if}
    <wa-button-group>
      <SyncButton variant="brand" onclick={startServer} disabled={busy || running}>
        Start server
      </SyncButton>
      <SyncButton onclick={stopServer} disabled={busy || !running}>Stop server</SyncButton>
    </wa-button-group>
  </section>

  <section class="block" aria-labelledby="sync-pairing-heading">
    <div class="block-header">
      <h3 id="sync-pairing-heading">Pairing</h3>
      <SyncButton variant="brand" onclick={beginPairing} disabled={busy}>Pair a device</SyncButton>
    </div>

    {#if ticket}
      <div class="copy-row">
        <wa-input readonly={true} label="Pairing URI" value={ticket.uri}></wa-input>
        <SyncButton onclick={() => copy('Pairing URI', ticket?.uri ?? '')}>Copy</SyncButton>
      </div>
      <div class="copy-row">
        <wa-input readonly={true} label="Pairing token" value={ticket.token}></wa-input>
        <SyncButton onclick={() => copy('Pairing token', ticket?.token ?? '')}>Copy</SyncButton>
      </div>
      <p class="muted">Expires {ticket.expires_at}.</p>
    {/if}

    <h4>Pending</h4>
    {#each pending as item (item.token)}
      <div class="row">
        <span>
          <strong>{item.device_name ?? 'Unknown device'}</strong>
          <span class="muted">· {item.device_type_id ?? 'unknown'} · {item.listen_on ?? '—'}</span>
        </span>
        <wa-button-group>
          <SyncButton variant="brand" onclick={() => approvePairing(item.token)} disabled={busy}>
            Approve
          </SyncButton>
          <SyncButton onclick={() => rejectPairing(item.token)} disabled={busy}>Reject</SyncButton>
        </wa-button-group>
      </div>
    {:else}
      <p class="muted">No pending pairings.</p>
    {/each}
  </section>

  <section class="block" aria-labelledby="sync-devices-heading">
    <div class="block-header">
      <h3 id="sync-devices-heading">Devices</h3>
    </div>
    {#each devices as device (device.device_id)}
      <div class="row">
        <span>
          <strong>{device.name ?? device.device_id}</strong>
          <span class="muted">
            · {device.device_type_id ?? 'unknown'} · paired {device.paired_at}
            {device.last_sync_at ? `· last sync ${device.last_sync_at}` : ''}
          </span>
        </span>
        <SyncButton onclick={() => revokeDevice(device.device_id)} disabled={busy}>
          Revoke
        </SyncButton>
      </div>
    {:else}
      <p class="muted">No paired devices.</p>
    {/each}
  </section>

  <section class="block" aria-labelledby="sync-conflicts-heading">
    <div class="block-header">
      <h3 id="sync-conflicts-heading">Conflicts</h3>
    </div>
    {#each conflicts as conflict (conflict.id)}
      <div class="conflict">
        <span>
          <strong>{conflict.entity_type} {conflict.entity_id}</strong>
          <span class="muted">· detected {conflict.detected_at}</span>
        </span>
        <div class="payloads">
          <pre>Local: {conflict.local_payload}</pre>
          <pre>Remote: {conflict.remote_payload}</pre>
        </div>
        {#if conflict.resolved}
          <p class="muted">Resolved as {conflict.resolution ?? 'unknown'}.</p>
        {:else}
          <div class="resolve">
            <label class="control">
              <span>Resolution</span>
              <select
                value={resolutions[conflict.id] ?? 'local'}
                onchange={(event) => {
                  resolutions[conflict.id] = event.currentTarget.value
                }}
              >
                <option value="local">Keep local</option>
                <option value="remote">Use remote</option>
                <option value="merged">Merge</option>
              </select>
            </label>
            {#if resolutions[conflict.id] === 'merged'}
              <label class="control">
                <span>Merged payload</span>
                <textarea
                  rows="3"
                  value={mergedPayloads[conflict.id] ?? ''}
                  oninput={(event) => {
                    mergedPayloads[conflict.id] = event.currentTarget.value
                  }}
                ></textarea>
              </label>
            {/if}
            <SyncButton variant="brand" onclick={() => resolveConflict(conflict)} disabled={busy}>
              Resolve
            </SyncButton>
          </div>
        {/if}
      </div>
    {:else}
      <p class="muted">No conflicts.</p>
    {/each}
  </section>

  <section class="block" aria-labelledby="sync-requests-heading">
    <div class="block-header">
      <h3 id="sync-requests-heading">Recent requests</h3>
    </div>
    {#each requests as record, index (record.at + record.path + index)}
      <div class="request">
        <span class="method">{record.method}</span>
        <span class="path">{record.path}</span>
        <wa-badge variant={record.status < 400 ? 'success' : 'danger'}>{record.status}</wa-badge>
        <span class="muted">{record.at}</span>
      </div>
    {:else}
      <p class="muted">No requests yet.</p>
    {/each}
  </section>
</div>

<style>
  .sync-panel {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    padding: var(--wa-space-l);
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-default);
  }

  .block-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
  }

  .block h3,
  .block h4 {
    margin: 0;
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
    gap: var(--wa-space-xs) var(--wa-space-l);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .facts li {
    color: var(--wa-color-text-secondary);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .row:first-of-type {
    border-top: none;
  }

  .copy-row {
    display: flex;
    align-items: flex-end;
    gap: var(--wa-space-s);
  }

  .copy-row wa-input {
    flex: 1 1 auto;
    font-family: var(--wa-font-family-code);
  }

  .conflict {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .conflict:first-of-type {
    border-top: none;
  }

  .payloads {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--wa-space-s);
  }

  .payloads pre {
    margin: 0;
    padding: var(--wa-space-s);
    overflow: auto;
    background: var(--wa-color-surface-alt);
    border-radius: var(--wa-border-radius);
    font-family: var(--wa-font-family-code);
    font-size: var(--wa-font-size-xs);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .resolve {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--wa-space-s);
  }

  .control {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .control select,
  .control textarea {
    padding: var(--wa-space-xs) var(--wa-space-s);
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-default);
    font: inherit;
  }

  .control select:focus-visible,
  .control textarea:focus-visible {
    outline: none;
    box-shadow: var(--wa-focus-ring);
  }

  .request {
    display: grid;
    grid-template-columns: 4rem 1fr auto auto;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
    font-size: var(--wa-font-size-s);
  }

  .request:first-of-type {
    border-top: none;
  }

  .method {
    font-family: var(--wa-font-family-code);
    text-transform: uppercase;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .muted {
    margin: 0;
    color: var(--wa-color-text-secondary);
  }
</style>
