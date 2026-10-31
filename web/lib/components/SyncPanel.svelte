<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import type { SyncConflict, SyncNearbyPeer, SyncPairingTicket } from '../bindings'
import { syncKeys } from '../query/keys'
import {
  listenToSyncEvents,
  syncConflictsList,
  syncConflictsResolve,
  syncDeviceNameGet,
  syncDeviceNameSave,
  syncDevicesList,
  syncDevicesRevoke,
  syncErrorMessage,
  syncHealth,
  syncLanGet,
  syncLanSet,
  syncNearbyDevices,
  syncPairClaim,
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

// Shorter than the bulk poll: discovery is what the pairing panel is for, and
// each call is already bounded by the daemon's listen window.
const NEARBY_POLL_MS = 5_000

const nearby = createQuery(() => ({
  queryKey: syncKeys.nearby(),
  queryFn: syncNearbyDevices,
  refetchInterval: NEARBY_POLL_MS,
}))

const deviceName = createQuery(() => ({
  queryKey: syncKeys.deviceName(),
  queryFn: syncDeviceNameGet,
}))

const lan = createQuery(() => ({
  queryKey: syncKeys.lan(),
  queryFn: syncLanGet,
}))

/**
 * The editable name. Seeded once from the store so a background refresh never
 * overwrites what the reader is typing.
 */
let nameDraft = $state('')
let nameSeeded = false
$effect(() => {
  const stored = deviceName.data
  if (nameSeeded || stored === undefined) return
  nameDraft = stored ?? ''
  nameSeeded = true
})

/** A pictogram for the peer's advertised `deviceFlavor`. */
function deviceEmoji(deviceType: string): string {
  switch (deviceType.toLowerCase()) {
    case 'ios':
    case 'iphone':
    case 'ipad':
      return '📱'
    case 'android':
      return '🤖'
    case 'desktop':
      return '🖥️'
    default:
      return '📡'
  }
}

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
const address = $derived(status.data ? `${status.data.host}:${status.data.port}` : null)

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

const saveName = createMutation(() => ({
  mutationFn: (name: string | null) => syncDeviceNameSave(name),
  onSuccess: (applied) => {
    queryClient.setQueryData(syncKeys.deviceName(), applied)
    nameDraft = applied ?? ''
    nameSeeded = true
    toast.success(applied ? 'Device name saved' : 'Device name reset')
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

function applyDeviceName() {
  const trimmed = nameDraft.trim()
  saveName.mutate(trimmed === '' ? null : trimmed)
}

const setLan = createMutation(() => ({
  mutationFn: (enabled: boolean) => syncLanSet(enabled),
  onSuccess: (enabled) => {
    queryClient.setQueryData(syncKeys.lan(), enabled)
    toast.success(
      enabled ? 'LAN sharing on — restart to apply' : 'LAN sharing off — restart to apply',
    )
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

function onLanToggle(event: Event) {
  setLan.mutate((event.currentTarget as WaSwitchElement).checked)
}

/**
 * The v2 pairing link: node id plus the advertised name.
 *
 * No address is embedded — a discovered peer resolves one over Bonjour, and
 * the manual entry sheet covers networks where multicast is blocked.
 */
function pairingLink(ticket: SyncPairingTicket): string {
  const parts = ['v=2', `nodeId=${encodeURIComponent(ticket.node_id)}`]
  const name = deviceName.data?.trim()
  if (name) parts.push(`name=${encodeURIComponent(name)}`)
  return `livtet://sync?${parts.join('&')}`
}

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

function approvePairing(nodeId: string) {
  action.mutate({ message: 'Device paired', run: () => syncPairingApprove(nodeId) })
}

function rejectPairing(nodeId: string) {
  action.mutate({ message: 'Pairing rejected', run: () => syncPairingReject(nodeId) })
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

/** Device whose claim is in flight, so its row can say so and block repeats. */
let pairingWith = $state<string | null>(null)

async function pairWith(peer: SyncNearbyPeer) {
  if (pairingWith) return
  const host = peer.addresses[0] ?? peer.hostname
  pairingWith = peer.node_id
  try {
    await syncPairClaim(`${host}:${peer.port}`, peer.node_id, peer.name)
    toast.info(`Pairing request sent to ${peer.name}`, {
      description: 'Approve it on that device, then its books can be pulled here.',
    })
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not start pairing')
  } finally {
    pairingWith = null
  }
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
    'sync://paired': (payload) => {
      invalidateQueries(syncKeys.devices(), syncKeys.nearby(), syncKeys.status())
      const name = (payload as { name?: string } | null)?.name
      toast.success(name ? `Paired with ${name}` : 'Device paired')
    },
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

<div class="sync">
  <header class="intro">
    <h2 id="sync-heading">Sync</h2>
    <p class="description">
      Keeps reading positions, shelves, and notes in step between paired devices.
      The daemon runs beside the library; its server is the endpoint the other
      device talks to.
    </p>
  </header>

  <section class="section" aria-labelledby="sync-status-heading">
    <div class="section-head">
      <h3 id="sync-status-heading">Server</h3>
      <ActionButton size="small" onclick={refreshAll} disabled={busy}>Refresh</ActionButton>
    </div>

    {#if health.data || status.data}
      <wa-callout variant={running ? 'success' : 'neutral'}>
        <wa-icon slot="icon" name={running ? 'circle-check' : 'circle-xmark'}></wa-icon>
        {#if running}
          <span class="status">
            {#if address}
              Sharing on <code>{address}</code>
              <ActionButton size="small" onclick={() => copy('Address', address ?? '')}>
                Copy
              </ActionButton>
            {:else}
              The server is running.
            {/if}
          </span>
          {#if address}
            <span class="hint">
              Pair a device below, then point it at this address to begin syncing.
            </span>
          {/if}
        {:else}
          Server stopped. The daemon is still reachable, so the library is not
          being published.
        {/if}
      </wa-callout>

      <dl class="facts">
        <div class="fact">
          <dt>Daemon</dt>
          <dd>
            {#if health.data}
              {health.data.status} <code>v{health.data.version}</code>
            {:else}
              unknown
            {/if}
          </dd>
        </div>
        <div class="fact">
          <dt>Device</dt>
          <dd><code>{status.data?.device_id ?? 'unknown'}</code></dd>
        </div>
        <div class="fact">
          <dt>Latest version</dt>
          <dd><code>{status.data?.latest_version ?? 0}</code></dd>
        </div>
        <div class="fact">
          <dt>Paired devices</dt>
          <dd>{status.data?.paired_device_count ?? 0}</dd>
        </div>
        <div class="fact">
          <dt>Pending pairings</dt>
          <dd>{status.data?.pending_pairing_count ?? 0}</dd>
        </div>
        <div class="fact">
          <dt>Requests served</dt>
          <dd>{status.data?.requests_served ?? 0}</dd>
        </div>
        <div class="fact">
          <dt>Last request</dt>
          <dd>{status.data?.last_request_at ?? 'never'}</dd>
        </div>
      </dl>

      <div class="actions">
        <ActionButton variant="brand" onclick={startServer} disabled={busy || running}>
          Start server
        </ActionButton>
        <ActionButton onclick={stopServer} disabled={busy || !running}>Stop server</ActionButton>
      </div>
    {:else}
      <p class="muted">Sync daemon is unavailable.</p>
    {/if}
  </section>

  <section class="section" aria-labelledby="sync-device-heading">
    <div class="section-head">
      <h3 id="sync-device-heading">This device</h3>
    </div>
    <p class="description">
      The name other devices see when they find this one. Leave it blank to
      generate a friendly name from the device id.
    </p>
    <div class="name-row">
      <wa-input
        class="device-name-input"
        label="Device name"
        placeholder="Generated name"
        value={nameDraft}
        oninput={(event) => (nameDraft = event.currentTarget.value)}
      ></wa-input>
      <ActionButton variant="brand" onclick={applyDeviceName} disabled={saveName.isPending}>
        Save
      </ActionButton>
    </div>
    <div class="name-row">
      <wa-switch
        aria-label="Accept sync peers from the LAN"
        checked={lan.data ?? true}
        disabled={setLan.isPending}
        onchange={onLanToggle}
      >
        Accept sync peers from the LAN
      </wa-switch>
    </div>
    <p class="description">
      On: the daemon binds every interface and advertises on the local network.
      Off: loopback only. Data routes still require a paired session, and this
      applies after the app restarts.
    </p>
  </section>

  <section class="section" aria-labelledby="sync-pairing-heading">
    <div class="section-head">
      <h3 id="sync-pairing-heading">Pairing</h3>
      <ActionButton size="small" variant="brand" onclick={beginPairing} disabled={busy}>
        Pair a device
      </ActionButton>
    </div>

    {#if ticket}
      <div class="ticket">
        <p class="muted">
          Scan this code on the other device, or enter the node ID, while the
          ticket is valid.
        </p>
        <div class="ticket-body">
          <div class="ticket-entry">
            <wa-input readonly={true} label="Node ID" value={ticket.node_id}></wa-input>
            <ActionButton onclick={() => copy('Node ID', ticket?.node_id ?? '')}>Copy</ActionButton>
          </div>
          <wa-qr-code
            class="ticket-qr"
            value={pairingLink(ticket)}
            label="Pairing link"
            size={160}
          ></wa-qr-code>
        </div>
        <p class="muted">Expires {ticket.expires_at}.</p>
      </div>
    {/if}

    <div class="nearby">
      <h4>Nearby devices</h4>
      {#if nearby.isPending}
        <p class="muted">Scanning the local network…</p>
      {:else if nearby.isError}
        <p class="muted">Nearby discovery is unavailable.</p>
      {:else if nearby.data?.length}
        {#each nearby.data as peer (peer.node_id)}
          <div class="row">
            <span class="nearby-emoji" aria-hidden="true">{deviceEmoji(peer.device_type)}</span>
            <span class="row-main">
              <strong>{peer.name}</strong>
              <span class="muted">
                · {peer.device_type} · {peer.addresses[0] ?? peer.hostname}:{peer.port}
              </span>
            </span>
            <wa-button-group>
              <ActionButton
                size="small"
                variant="brand"
                onclick={() => pairWith(peer)}
                disabled={pairingWith !== null}
              >
                {pairingWith === peer.node_id ? 'Waiting…' : 'Pair'}
              </ActionButton>
              <ActionButton size="small" onclick={() => copy('Node ID', peer.node_id)}>
                Copy ID
              </ActionButton>
            </wa-button-group>
          </div>
        {/each}
      {:else}
        <p class="muted">No devices found on this network.</p>
      {/if}
    </div>

    <h4>Waiting for approval</h4>
    {#each pending.data ?? [] as item (item.node_id)}
      <div class="row">
        <span class="row-main">
          <strong>{item.device_name ?? 'Unknown device'}</strong>
          <span class="muted">· {item.device_type_id ?? 'unknown'} · {item.listen_on ?? '—'}</span>
        </span>
        <wa-button-group>
          <ActionButton size="small" variant="brand" onclick={() => approvePairing(item.node_id)} disabled={busy}>
            Approve
          </ActionButton>
          <ActionButton size="small" onclick={() => rejectPairing(item.node_id)} disabled={busy}>
            Reject
          </ActionButton>
        </wa-button-group>
      </div>
    {:else}
      <p class="muted">No pending pairings.</p>
    {/each}
  </section>

  <section class="section" aria-labelledby="sync-devices-heading">
    <div class="section-head">
      <h3 id="sync-devices-heading">Devices</h3>
      {#if status.data?.paired_device_count}
        <wa-badge variant="neutral">{status.data.paired_device_count}</wa-badge>
      {/if}
    </div>
    {#each devices.data ?? [] as device (device.device_id)}
      <div class="row">
        <span class="row-main">
          <strong>{device.name ?? device.device_id}</strong>
          <span class="muted">
            · {device.device_type_id ?? 'unknown'} · paired {device.paired_at}
            {device.last_sync_at ? `· last sync ${device.last_sync_at}` : ''}
          </span>
        </span>
        <ActionButton size="small" onclick={() => revokeDevice(device.device_id)} disabled={busy}>
          Revoke
        </ActionButton>
      </div>
    {:else}
      <p class="muted">No paired devices.</p>
    {/each}
  </section>

  <section class="section" aria-labelledby="sync-conflicts-heading">
    <div class="section-head">
      <h3 id="sync-conflicts-heading">Conflicts</h3>
      {#if conflicts.data?.length}
        <wa-badge variant="warning">{conflicts.data.length}</wa-badge>
      {/if}
    </div>
    {#each conflicts.data ?? [] as conflict (conflict.id)}
      <div class="conflict">
        <span class="conflict-head">
          <strong>{conflict.entity_type} {conflict.entity_id}</strong>
          <span class="muted">· detected {conflict.detected_at}</span>
        </span>
        <div class="payloads">
          <div class="payload">
            <span class="label">Local</span>
            <pre>{conflict.local_payload}</pre>
          </div>
          <div class="payload">
            <span class="label">Remote</span>
            <pre>{conflict.remote_payload}</pre>
          </div>
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

  <section class="section" aria-labelledby="sync-requests-heading">
    <div class="section-head">
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
  .sync {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .intro {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
  }

  .intro h2 {
    margin: 0;
  }

  .description {
    margin: 0;
    max-width: 65ch;
    color: var(--wa-color-text-quiet);
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    padding-block-start: var(--wa-space-l);
    border-block-start: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
  }

  .section-head {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
  }

  .section-head h3 {
    margin: 0;
  }

  .section h4 {
    margin: var(--wa-space-s) 0 0;
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
    font-weight: var(--wa-font-weight-medium);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--wa-space-s);
    flex-wrap: wrap;
  }

  .hint {
    display: block;
    margin-block-start: var(--wa-space-2xs);
    color: var(--wa-color-text-quiet);
  }

  /* The counts read as one scannable block instead of a nine-row table. */
  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
    gap: var(--wa-space-s) var(--wa-space-l);
    margin: 0;
  }

  .fact {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    min-width: 0;
  }

  .fact dt {
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-xs);
  }

  .fact dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
  }

  .name-row {
    display: flex;
    align-items: flex-end;
    gap: var(--wa-space-s);
    flex-wrap: wrap;
  }

  .name-row wa-input {
    flex: 1 1 18rem;
    max-width: 28rem;
  }

  .ticket {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    padding: var(--wa-space-m);
    border: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
    border-radius: var(--wa-radius-m);
    background: var(--wa-color-surface-alt);
  }

  .ticket-body {
    display: flex;
    align-items: flex-start;
    gap: var(--wa-space-l);
    flex-wrap: wrap;
  }

  .ticket-entry {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--wa-space-s);
    flex: 1 1 16rem;
    min-width: 0;
  }

  .ticket-entry wa-input {
    width: 100%;
  }

  .ticket-entry wa-input::part(input) {
    font-family: var(--wa-font-family-code);
  }

  /* The QR sits on its own white field: it must stay scannable in either
     theme, since the component draws black modules on a fixed backdrop. */
  .ticket-qr {
    flex: none;
    padding: var(--wa-space-2xs);
    border-radius: var(--wa-radius-m);
    background: #fff;
  }

  .nearby {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .nearby-emoji {
    flex: none;
    font-size: 1.25rem;
    line-height: 1;
  }

  /* The name reads next to its pictogram; only the Copy button is pushed out. */
  .nearby .row {
    justify-content: flex-start;
  }

  .nearby .row-main {
    flex: 1 1 auto;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding-block: var(--wa-space-s);
    border-block-start: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
  }

  .row:first-of-type {
    border-block-start: none;
  }

  .row-main {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--wa-space-2xs);
    min-width: 0;
  }

  .conflict {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    padding-block: var(--wa-space-s);
    border-block-start: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
  }

  .conflict:first-of-type {
    border-block-start: none;
  }

  .conflict-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--wa-space-2xs);
  }

  .payloads {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
    gap: var(--wa-space-s);
  }

  .payload {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    min-width: 0;
  }

  .label {
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-xs);
  }

  .payload pre {
    margin: 0;
    padding: var(--wa-space-s);
    overflow: auto;
    background: var(--wa-color-surface-alt);
    border-radius: var(--wa-radius-m);
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
    color: var(--wa-color-text-quiet);
  }

  .control select,
  .control textarea {
    padding: var(--wa-space-xs) var(--wa-space-s);
    border: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
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
    grid-template-columns: 4.5rem minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--wa-space-s);
    padding-block: var(--wa-space-xs);
    border-block-start: var(--wa-border-width-s) var(--wa-border-style)
      var(--wa-color-surface-border);
    font-size: var(--wa-font-size-s);
  }

  .request:first-of-type {
    border-block-start: none;
  }

  .method {
    font-family: var(--wa-font-family-code);
    font-size: var(--wa-font-size-xs);
    text-transform: uppercase;
  }

  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .muted {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }
</style>
