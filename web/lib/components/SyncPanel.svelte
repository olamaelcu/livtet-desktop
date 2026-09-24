<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import type { SyncConflict, SyncPairingTicket } from '../bindings'
import { syncKeys } from '../query/keys'
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
import ActionButton from './ActionButton.svelte'

const SYNC_POLL_MS = 30_000
const REQUEST_LIMIT = 50

const queryClient = useQueryClient()

function invalidateQueries(...queryKeys: ReadonlyArray<readonly unknown[]>) {
  for (const queryKey of queryKeys) queryClient.invalidateQueries({ queryKey })
}

const health = createQuery(() => ({
  queryKey: syncKeys.health(),
  queryFn: syncHealth,
  refetchInterval: SYNC_POLL_MS,
}))

const status = createQuery(() => ({
  queryKey: syncKeys.status(),
  queryFn: syncStatus,
  refetchInterval: SYNC_POLL_MS,
}))

const pending = createQuery(() => ({
  queryKey: syncKeys.pending(),
  queryFn: syncPairingList,
  refetchInterval: SYNC_POLL_MS,
}))

const devices = createQuery(() => ({
  queryKey: syncKeys.devices(),
  queryFn: syncDevicesList,
  refetchInterval: SYNC_POLL_MS,
}))

const conflicts = createQuery(() => ({
  queryKey: syncKeys.conflicts(),
  queryFn: syncConflictsList,
  refetchInterval: SYNC_POLL_MS,
}))

const requests = createQuery(() => ({
  queryKey: syncKeys.requests(REQUEST_LIMIT),
  queryFn: () => syncRequestsRecent(REQUEST_LIMIT),
  refetchInterval: SYNC_POLL_MS,
}))

const running = $derived(status.data?.server_running ?? health.data?.server_running ?? false)

type SyncAction = { message: string; run: () => Promise<unknown> }

const action = createMutation(() => ({
  mutationFn: (variables: SyncAction) => variables.run(),
  onSuccess: (_data, variables) => {
    toast.success(variables.message)
    invalidateQueries(syncKeys.all)
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

let ticket = $state<SyncPairingTicket | null>(null)
let resolutions = $state<Record<number, string | undefined>>({})
let mergedPayloads = $state<Record<number, string | undefined>>({})

const pairing = createMutation(() => ({
  mutationFn: () => syncPairingBegin(),
  onSuccess: (data) => {
    ticket = data
    toast.success('Pairing ticket created')
    invalidateQueries(syncKeys.status())
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

const busy = $derived(action.isPending || pairing.isPending)

function refreshAll() {
  return invalidateQueries(syncKeys.all)
}

function startServer() {
  action.mutate({ message: 'Sync server started', run: syncServerStart })
}

function stopServer() {
  action.mutate({ message: 'Sync server stopped', run: syncServerStop })
}

function beginPairing() {
  pairing.mutate()
}

function approvePairing(token: string) {
  action.mutate({ message: 'Device paired', run: () => syncPairingApprove(token) })
}

function rejectPairing(token: string) {
  action.mutate({ message: 'Pairing rejected', run: () => syncPairingReject(token) })
}

function revokeDevice(deviceId: string) {
  action.mutate({ message: 'Device revoked', run: () => syncDevicesRevoke(deviceId) })
}

function resolveConflict(conflict: SyncConflict) {
  const resolution = resolutions[conflict.id] ?? 'local'
  const merged = resolution === 'merged' ? (mergedPayloads[conflict.id] ?? '') : undefined
  action.mutate({
    message: 'Conflict resolved',
    run: () => syncConflictsResolve(conflict.id, resolution, merged),
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
    'sync://pairing-requested': () => invalidateQueries(syncKeys.pending(), syncKeys.status()),
    'sync://request': () => invalidateQueries(syncKeys.requests(REQUEST_LIMIT), syncKeys.status()),
    'sync://completed': () => invalidateQueries(syncKeys.status(), syncKeys.conflicts()),
    'sync://server-started': () => invalidateQueries(syncKeys.health(), syncKeys.status()),
    'sync://server-stopped': () => invalidateQueries(syncKeys.health(), syncKeys.status()),
  }).then((stop) => {
    if (disposed) stop()
    else unlisten = stop
  })

  return () => {
    disposed = true
    unlisten?.()
  }
})
</script>

<div class="sync-panel">
  <section class="block" aria-labelledby="sync-status-heading">
    <div class="block-header">
      <h3 id="sync-status-heading">Status</h3>
      <ActionButton onclick={refreshAll} disabled={busy}>Refresh</ActionButton>
    </div>
    {#if health.data || status.data}
      <table class="facts">
        <thead>
          <tr>
            <th>Status</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row">Daemon</th>
            <td>{@html health.data ? `${health.data.status} <code>v${health.data.version}</code>` : 'unknown'}</td>
          </tr>
          <tr>
            <th scope="row">Device</th>
            <td><code>{status.data?.device_id ?? 'unknown'}</code></td>
          </tr>
          <tr>
            <th scope="row">Address</th>
            <td><code>{status.data ? `${status.data.host}:${status.data.port}` : 'unknown'}</code></td>
          </tr>
          <tr>
            <th scope="row">Server</th>
            <td>{running ? 'running' : 'stopped'}</td>
          </tr>
          <tr>
            <th scope="row">Latest version</th>
            <td><code>{status.data?.latest_version ?? 0}</code></td>
          </tr>
          <tr>
            <th scope="row">Paired devices</th>
            <td>{status.data?.paired_device_count ?? 0}</td>
          </tr>
          <tr>
            <th scope="row">Pending pairings</th>
            <td>{status.data?.pending_pairing_count ?? 0}</td>
          </tr>
          <tr>
            <th scope="row">Requests served</th>
            <td>{status.data?.requests_served ?? 0}</td>
          </tr>
          <tr>
            <th scope="row">Last request</th>
            <td>{status.data?.last_request_at ?? 'never'}</td>
          </tr>
        </tbody>
      </table>
    {:else}
      <p class="muted">Sync daemon is unavailable.</p>
    {/if}
    <wa-button-group>
      <ActionButton variant="brand" onclick={startServer} disabled={busy || running}>
        Start server
      </ActionButton>
      <ActionButton onclick={stopServer} disabled={busy || !running}>Stop server</ActionButton>
    </wa-button-group>
  </section>

  <section class="block" aria-labelledby="sync-pairing-heading">
    <div class="block-header">
      <h3 id="sync-pairing-heading">Pairing</h3>
      <ActionButton variant="brand" onclick={beginPairing} disabled={busy}>Pair a device</ActionButton>
    </div>

    {#if ticket}
      <div class="code-region">
      <wa-qr-code
        class="pairing-qr"
        value={ticket.uri}
        label="Scan to pair a device"
        size={160}
      ></wa-qr-code>
<div>
      <div class="copy-row">
        <wa-input readonly={true} label="URI" value={ticket.uri}></wa-input>
        <ActionButton onclick={() => copy('Pairing URI', ticket?.uri ?? '')}>Copy</ActionButton>
      </div>
      <div class="copy-row">
        <wa-input readonly={true} label="Token" value={ticket.token}></wa-input>
        <ActionButton onclick={() => copy('Pairing token', ticket?.token ?? '')}>Copy</ActionButton>
      </div>
</div>
</div>
      <p class="muted">Expires {ticket.expires_at}.</p>
    {/if}

    <h4>Pending</h4>
    {#each pending.data ?? [] as item (item.token)}
      <div class="row">
        <span>
          <strong>{item.device_name ?? 'Unknown device'}</strong>
          <span class="muted">· {item.device_type_id ?? 'unknown'} · {item.listen_on ?? '—'}</span>
        </span>
        <wa-button-group>
          <ActionButton variant="brand" onclick={() => approvePairing(item.token)} disabled={busy}>
            Approve
          </ActionButton>
          <ActionButton onclick={() => rejectPairing(item.token)} disabled={busy}>Reject</ActionButton>
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
    {#each devices.data ?? [] as device (device.device_id)}
      <div class="row">
        <span>
          <strong>{device.name ?? device.device_id}</strong>
          <span class="muted">
            · {device.device_type_id ?? 'unknown'} · paired {device.paired_at}
            {device.last_sync_at ? `· last sync ${device.last_sync_at}` : ''}
          </span>
        </span>
        <ActionButton onclick={() => revokeDevice(device.device_id)} disabled={busy}>
          Revoke
        </ActionButton>
      </div>
    {:else}
      <p class="muted">No paired devices.</p>
    {/each}
  </section>

  <section class="block" aria-labelledby="sync-conflicts-heading">
    <div class="block-header">
      <h3 id="sync-conflicts-heading">Conflicts</h3>
    </div>
    {#each conflicts.data ?? [] as conflict (conflict.id)}
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
            <ActionButton variant="brand" onclick={() => resolveConflict(conflict)} disabled={busy}>
              Resolve
            </ActionButton>
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
    {#each requests.data ?? [] as record, index (record.at + record.path + index)}
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
    width: 100%;
    border-collapse: collapse;
    font-size: var(--wa-font-size-s);
  }

  .facts th,
  .facts td {
    padding: var(--wa-space-xs) 0;
    text-align: left;
    vertical-align: top;
  }

  .facts thead th {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    font-weight: var(--wa-font-weight-medium);
  }

  .facts tbody th {
    color: var(--wa-color-text-secondary);
    width: 40%;
  }

  .facts tbody tr:first-child th,
  .facts tbody tr:first-child td {
    padding-top: 0;
  }

  .facts tbody tr:last-child th,
  .facts tbody tr:last-child td {
    padding-bottom: 0;
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

  .code-region {
    display: flex;
    align-items: center;

    > div {
      display: flex;
      flex-direction: column;
      gap: var(--wa-space-s);
      flex: 1 1;
    }
  }

  .pairing-qr {
    align-self: flex-start;
    padding: var(--wa-space-m);
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
    background: var(--wa-color-surface-default);
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
