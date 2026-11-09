<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import type { SyncConflict, SyncNearbyPeer, SyncPairingTicket } from '../bindings'
import ConfirmDialog from '../library/ConfirmDialog.svelte'
import { syncKeys } from '../query/keys'
import {
  listenToSyncEvents,
  syncConflictsList,
  syncConflictsResolve,
  syncDeviceNameGet,
  syncDeviceNameSave,
  syncDevicesList,
  syncDevicesRevoke,
  syncDiagnostics,
  syncErrorMessage,
  syncFirewallAllow,
  syncFirewallHint,
  syncHealth,
  syncNearbyDevices,
  syncPairClaim,
  syncPairingApprove,
  syncPairingBegin,
  syncPairingList,
  syncPairingReject,
  syncPull,
  syncStatus,
} from '../sync'
import ActionButton from './ActionButton.svelte'

const CROCKFORD = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'

/** Paired rows expose the device id as a ULID; discovery advertises the same bytes as hex. */
function ulidToHex(id: string): string {
  if (!/^[0-9A-HJKMNP-TV-Z]{26}$/.test(id)) return id.toLowerCase()
  let value = 0n
  for (const char of id) value = value * 32n + BigInt(CROCKFORD.indexOf(char))
  return value.toString(16).padStart(32, '0')
}

const SYNC_POLL_MS = 30_000

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

/**
 * Sum up this device's pairing state and the peers it holds.
 *
 * A mutation rather than a query: it is a snapshot the reader asks for, and
 * its findings would otherwise be refetched while they are being read.
 */
const diagnose = createMutation(() => ({
  mutationFn: syncDiagnostics,
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

/**
 * The firewall rule a peer would need to reach this server.
 *
 * Only asked once the daemon reports a port: the rule is for that port, and a
 * query against 0 would only produce an error.
 */
const firewall = createQuery(() => ({
  queryKey: syncKeys.firewall(),
  queryFn: () => syncFirewallHint(status.data?.port ?? 0),
  enabled: Boolean(status.data?.port),
}))

/**
 * The port the reader has already allowed this session.
 *
 * The host firewall's rule cannot be read back without elevation, so a
 * successful `allow` is the only evidence the callout has been dealt with;
 * hiding it for that port keeps the notice from outliving its purpose.
 */
let allowedPort = $state<number | null>(null)

const allowFirewall = createMutation(() => ({
  mutationFn: (port: number) => syncFirewallAllow(port),
  onSuccess: (_result, port) => {
    allowedPort = port
    toast.success('Allowed other devices to connect')
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

const conflicts = createQuery(() => ({
  queryKey: syncKeys.conflicts(),
  queryFn: syncConflictsList,
  refetchInterval: SYNC_POLL_MS,
}))

/** Device ids this desktop already holds a row for, so discovery can mark them. */
const pairedIds = $derived(
  new Set(
    (devices.data ?? []).flatMap((device) => [device.device_id, ulidToHex(device.device_id)]),
  ),
)

/** Discovery also sees devices that are already paired; the list is for pairing, so omit them. */
const unpairedNearby = $derived(
  (nearby.data ?? []).filter((peer) => !pairedIds.has(peer.device_id)),
)

/** Only unresolved rows can be acted on; resolved ones stay visible as history. */
const unresolvedConflicts = $derived(
  (conflicts.data ?? []).filter((conflict) => !conflict.resolved),
)

/** Notes merge automatically in Stage B; until then their row is read-only. */
function isNoteConflict(conflict: SyncConflict): boolean {
  return conflict.entity_type === 'reading_progress'
}

const resolvableConflicts = $derived(unresolvedConflicts.filter((conflict) => !isNoteConflict(conflict)))

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
let bulkResolution = $state<'local' | 'remote'>('local')
let bulkConfirmOpen = $state(false)

const pairing = createMutation(() => ({
  mutationFn: () => syncPairingBegin(),
  onSuccess: (data) => {
    ticket = data
    toast.success('Pairing ticket created')
    invalidateQueries(syncKeys.status())
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

const bulkResolve = createMutation(() => ({
  mutationFn: async (resolution: 'local' | 'remote') => {
    const targets = resolvableConflicts.map((conflict) => conflict.id)
    let resolved = 0
    for (const id of targets) {
      if (await syncConflictsResolve(id, resolution)) resolved += 1
    }
    return { resolved, total: targets.length, resolution }
  },
  onSuccess: ({ resolved, total, resolution }) => {
    const choice = resolution === 'local' ? 'local' : 'remote'
    toast.success(`Resolved ${resolved} of ${total} conflicts as ${choice}`)
    invalidateQueries(syncKeys.all)
  },
  onError: (error) => {
    toast.error(syncErrorMessage(error))
    // A failed row may still have resolved earlier rows; refresh rather than
    // leaving the reader with a stale list.
    invalidateQueries(syncKeys.conflicts(), syncKeys.status())
  },
}))

const busy = $derived(action.isPending || pairing.isPending || bulkResolve.isPending)

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

/**
 * The v2 pairing link: the pairing code plus the advertised name.
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

/** Revoke both sides, then claim again so the pair gets fresh mutual tokens. */
async function repairDevice(device: { device_id: string; name: string | null }) {
  const peers = await syncNearbyDevices().catch(() => [])
  const target = peers.find(
    (peer) => peer.device_id === ulidToHex(device.device_id) || peer.name === device.name,
  )
  action.mutate({
    message: 'Repair request sent',
    run: async () => {
      await syncDevicesRevoke(device.device_id)
      if (!target) {
        throw new Error(
          'Revoked locally, but the device is not nearby; pair again when it appears.',
        )
      }
      await syncPairClaim(target.node_id, device.name)
    },
  })
}

type PullReport = { name: string; offered: number; error?: string }

/**
 * The device being pulled from, and when that started.
 *
 * A pull waits on the peer — a transfer, a file write, then a merge — so it can
 * run for minutes. Without this the button says "Checking…" for that whole
 * time, which reads as a hang.
 */
let pullProgress = $state<{ device: string; startedAt: number } | null>(null)
let clock = $state(Date.now())

$effect(() => {
  if (!pullProgress) return
  const tick = setInterval(() => (clock = Date.now()), 1000)
  return () => clearInterval(tick)
})

const pullElapsed = $derived(
  pullProgress ? Math.max(0, Math.floor((clock - pullProgress.startedAt) / 1000)) : 0,
)

/**
 * Pull every device this desktop can read.
 *
 * A book offered to this desktop only exists in the sender's change log, so
 * this is what makes it appear here. Pairing is mutual, so a device this
 * desktop paired with is pullable in both directions; only a pairing approved
 * before that was true is still send-only, and the daemon refuses to pull it.
 */
const pull = createMutation(() => ({
  mutationFn: async (): Promise<PullReport[]> => {
    const reports: PullReport[] = []
    try {
      for (const device of devices.data ?? []) {
        if (!device.outbound) continue
        const name = device.name ?? device.device_id
        pullProgress = { device: name, startedAt: Date.now() }
        try {
          const outcome = await syncPull(device.device_id)
          reports.push({ name, offered: outcome.offered })
        } catch (error) {
          reports.push({ name, offered: 0, error: syncErrorMessage(error) })
        }
      }
    } finally {
      pullProgress = null
    }
    return reports
  },
  onSuccess: (reports) => {
    invalidateQueries(
      syncKeys.offers(),
      syncKeys.devices(),
      syncKeys.status(),
      syncKeys.conflicts(),
    )
    if (!reports.length) {
      // Nothing was pulled, but the Devices list above may still show rows: an
      // approved device is a real pairing that points the other way. Say which
      // ones, or the message reads as a contradiction of the list.
      const paired = (devices.data ?? []).map((device) => device.name ?? device.device_id)
      if (paired.length) {
        toast.info(`Nothing can be pulled from ${paired.join(', ')}`, {
          description:
            'Those devices paired to this desktop, so they may pull your books but not the reverse. Claim one from Nearby devices with Re-pair to receive its books.',
        })
      } else {
        toast.info('No paired devices yet', {
          description: 'Pair a device from Nearby devices to sync with it.',
        })
      }
      return
    }
    const offered = reports.reduce((total, report) => total + report.offered, 0)
    const failed = reports.filter((report) => report.error)
    if (offered > 0) {
      toast.success(`Received ${offered} offered ${offered === 1 ? 'book' : 'books'}`)
    } else if (!failed.length) {
      toast.info('Nothing new from your paired devices')
    }
    if (failed.length) {
      toast.error(`Could not pull from ${failed.map((report) => report.name).join(', ')}`, {
        description: failed[0]?.error,
      })
    }
  },
  onError: (error) => toast.error(syncErrorMessage(error)),
}))

function resolveConflict(conflict: SyncConflict) {
  const resolution = resolutions[conflict.id] ?? 'local'
  const merged = resolution === 'merged' ? (mergedPayloads[conflict.id] ?? '') : undefined
  action.mutate({
    message: 'Conflict resolved',
    run: () => syncConflictsResolve(conflict.id, resolution, merged),
  })
}

/** Resolve every unresolved conflict with one choice; merged needs per-row input. */
function resolveAllConflicts() {
  if (!resolvableConflicts.length || bulkResolve.isPending) return
  bulkConfirmOpen = true
}

function confirmResolveAll() {
  bulkConfirmOpen = false
  bulkResolve.mutate(bulkResolution)
}

/** Device whose claim is in flight, so its row can say so and block repeats. */
let pairingWith = $state<string | null>(null)

async function pairWith(peer: SyncNearbyPeer) {
  if (pairingWith) return
  pairingWith = peer.node_id
  try {
    await syncPairClaim(peer.node_id, peer.name)
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
    // A pull this device made applied rows here, so the catalog and any
    // conflicts it raised are both stale.
    'sync://pulled': () => invalidateQueries(syncKeys.status(), syncKeys.conflicts()),
    'sync://offer-received': (payload) => {
      // A pull records offers as it finds them, so this is live progress even
      // while the pull's own reply is still pending.
      invalidateQueries(syncKeys.offers())
      const title = (payload as { title?: string } | null)?.title
      toast.info(title ? `A book was offered: ${title}` : 'A book was offered')
    },
    'sync://paired': (payload) => {
      invalidateQueries(syncKeys.devices(), syncKeys.nearby(), syncKeys.status())
      const name = (payload as { name?: string } | null)?.name
      toast.success(name ? `Paired with ${name}` : 'Device paired')
    },
    // A peer answered an offer this device sent; the sender's offer rows and
    // its peer list both move.
    'sync://offer-decided': () => invalidateQueries(syncKeys.offers(), syncKeys.devices()),
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
      The daemon runs quietly beside the library.
    </p>
  </header>

  <section class="section" aria-labelledby="sync-status-heading">
    <div class="section-head">
      <h3 id="sync-status-heading">Daemon</h3>
      <ActionButton size="small" onclick={refreshAll} disabled={busy}>Refresh</ActionButton>
    </div>

    {#if health.data || status.data}
      <wa-callout variant="success">
        <wa-icon slot="icon" name="circle-check"></wa-icon>
        <span class="status">Sync is on.</span>
        <span class="hint">Paired devices stay in step automatically.</span>
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
          <dt>Device ID</dt>
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
      </dl>
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
          Scan this code on the other device, or enter the pairing code, while
          the ticket is valid.
        </p>
        <div class="ticket-body">
          <div class="ticket-entry">
            <wa-input readonly={true} label="Pairing code" value={ticket.node_id}></wa-input>
            <ActionButton onclick={() => copy('Pairing code', ticket?.node_id ?? '')}>
              Copy
            </ActionButton>
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
      <p class="description">
        Pairing from here claims the other device, which is what lets this one
        pull its books. Approving a device instead only lets it pull from this
        one, so re-pair from here to receive its books.
      </p>
      {#if nearby.isPending}
        <p class="muted">Looking for nearby devices…</p>
      {:else if nearby.isError}
        <p class="muted">Nearby discovery is unavailable.</p>
      {:else if unpairedNearby.length}
        {#each unpairedNearby as peer (peer.node_id)}
          <div class="row">
            <span class="nearby-emoji" aria-hidden="true">{deviceEmoji(peer.device_type)}</span>
            <span class="row-main">
              <strong>{peer.name}</strong>
              <span class="muted">
                · {peer.device_type}
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
              <ActionButton size="small" onclick={() => copy('Pairing code', peer.node_id)}>
                Copy code
              </ActionButton>
            </wa-button-group>
          </div>
        {/each}
      {:else}
        <p class="muted">No nearby devices found.</p>
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
      <div class="section-actions">
        {#if status.data?.paired_device_count}
          <wa-badge variant="neutral">{status.data.paired_device_count}</wa-badge>
        {/if}
        <ActionButton
          size="small"
          variant="brand"
          onclick={() => pull.mutate()}
          disabled={pull.isPending || !devices.data?.length}
        >
          {#if pull.isPending}
            {pullProgress ? `Pulling from ${pullProgress.device}… ${pullElapsed}s` : 'Checking…'}
          {:else}
            Check for books
          {/if}
        </ActionButton>
      </div>
    </div>
    <p class="description">
      Pairing is mutual: either device can read the other's library, and both
      pull about once a minute. A device marked <strong>One-way</strong> can
      read this library, but this device cannot read it until the two are paired
      again.
    </p>
    {#each devices.data ?? [] as device (device.device_id)}
      <div class="row">
        <span class="row-main">
          <strong>{device.name ?? device.device_id}</strong>
          <wa-badge variant={device.outbound ? 'success' : 'neutral'}>
            {device.outbound ? 'Two-way' : 'One-way'}
          </wa-badge>
          <span class="muted">
            {device.device_type_id ?? 'Unknown'} · paired {device.paired_at}
            {device.last_sync_at ? `· last sync ${device.last_sync_at}` : ''}
            {device.outbound ? '' : '· pair again to read its books'}
          </span>
        </span>
        <ActionButton
          size="small"
          variant="brand"
          onclick={() => repairDevice(device)}
          disabled={busy}
        >
          Repair
        </ActionButton>
        <ActionButton size="small" onclick={() => revokeDevice(device.device_id)} disabled={busy}>
          Revoke
        </ActionButton>
      </div>
    {:else}
      <p class="muted">No paired devices.</p>
    {/each}
  </section>

  <section class="section" aria-labelledby="sync-reach-heading">
    <div class="section-head">
      <h3 id="sync-reach-heading">Sync check</h3>
      <div class="section-actions">
        <ActionButton
          size="small"
          onclick={() => diagnose.mutate()}
          disabled={diagnose.isPending}
        >
          {diagnose.isPending ? 'Checking…' : 'Check'}
        </ActionButton>
      </div>
    </div>
    <p class="description">
      If a paired device is not keeping in step, check reports what is getting
      in the way: a request that was never approved, or a pairing that works
      only one way.
    </p>
    {#if firewall.data && firewall.data.port !== allowedPort}
      <wa-callout variant="warning">
        <wa-icon slot="icon" name="shield-halved"></wa-icon>
        <p>
          <strong>{firewall.data.tool}</strong> is enabled on this machine and blocks
          connections from other devices, so a paired device cannot sync with this
          one.
          {#if firewall.data.automatic}
            Allowing it asks your system for permission.
          {:else}
            Run the command below in a terminal; this build cannot elevate.
          {/if}
        </p>
        <div class="firewall-rule">
          <code>{firewall.data.command}</code>
          <ActionButton
            size="small"
            onclick={() => copy('Command', firewall.data?.command ?? '')}
          >
            Copy
          </ActionButton>
          {#if firewall.data.automatic}
            <ActionButton
              size="small"
              variant="brand"
              onclick={() => allowFirewall.mutate(firewall.data?.port ?? 0)}
              disabled={allowFirewall.isPending}
            >
              {allowFirewall.isPending ? 'Waiting…' : 'Allow'}
            </ActionButton>
          {/if}
        </div>
      </wa-callout>
    {/if}
    {#if diagnose.data}
      <ul class="findings">
        {#each diagnose.data.findings as finding, index (index)}
          <li>{finding}</li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="section" aria-labelledby="sync-conflicts-heading">
    <div class="section-head">
      <h3 id="sync-conflicts-heading">Conflicts</h3>
      {#if unresolvedConflicts.length}
        <wa-badge variant="warning">{unresolvedConflicts.length}</wa-badge>
      {/if}
    </div>
    {#if resolvableConflicts.length}
      <div class="resolve bulk-resolve">
        <label class="control">
          <span>Resolve all as</span>
          <select
            value={bulkResolution}
            onchange={(event) => {
              bulkResolution = event.currentTarget.value as 'local' | 'remote'
            }}
          >
            <option value="local">Keep local</option>
            <option value="remote">Use remote</option>
          </select>
        </label>
        <ActionButton
          variant="brand"
          onclick={resolveAllConflicts}
          disabled={busy}
          size="small"
        >
          Resolve all ({resolvableConflicts.length})
        </ActionButton>
      </div>
    {/if}
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
        {:else if isNoteConflict(conflict)}
          <p class="muted">
            Notes merge automatically in the next update; no action is needed here.
          </p>
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
    <ConfirmDialog
      open={bulkConfirmOpen}
      title="Resolve conflicts"
      message={`Resolve ${unresolvedConflicts.length} conflicts as “${bulkResolution === 'local' ? 'Keep local' : 'Use remote'}”? This cannot be undone.`}
      confirmLabel="Resolve all"
      onconfirm={confirmResolveAll}
      oncancel={() => {
        bulkConfirmOpen = false
      }}
    />
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

  .firewall-rule {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    flex-wrap: wrap;
  }
  .findings {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    margin: 0;
    padding-inline-start: var(--wa-space-l);
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
  .section-actions {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    margin-left: auto;
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
    align-items: center;
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

  .bulk-resolve {
    padding-block: var(--wa-space-s);
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

  .muted {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }
</style>
