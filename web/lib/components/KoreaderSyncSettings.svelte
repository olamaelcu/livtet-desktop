<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import type { KoreaderSyncSettings as KoreaderSyncConfig } from '../bindings'
import {
  getKoreaderSyncStatus,
  koreaderSyncErrorMessage,
  saveKoreaderSyncSettings,
} from '../koreader-sync'
import { koreaderSyncKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const status = createQuery(() => ({
  queryKey: koreaderSyncKeys.status(),
  queryFn: getKoreaderSyncStatus,
}))

let enabled = $state(false)
let port = $state('8081')
let lan = $state(true)
let username = $state('')
let password = $state('')
// Plain (not `$state`) so it is not an effect dependency: the form takes its
// initial values from the backend once, then belongs to the user until Save.
let seeded = false
// Whether the user typed a password this session. Untouched means "keep the
// stored one"; the backend never sends the stored password back.
// `$state`, not a plain flag: `credentialsValid` below reads it, and a plain
// variable would leave that derived value cached at its first answer.
let passwordTouched = $state(false)
let switchControl = $state<WaSwitchElement>()

$effect(() => {
  const current = status.data
  if (seeded || !current) return
  enabled = current.settings.enabled
  port = String(current.settings.port)
  lan = current.settings.lan
  username = current.settings.username ?? ''
  password = current.settings.password ?? ''
  seeded = true
})

const parsedPort = $derived(Number(port))
const portValid = $derived(Number.isInteger(parsedPort) && parsedPort >= 1 && parsedPort <= 65535)
const credentialsValid = $derived(
  username.trim().length > 0 &&
    (passwordTouched ? password.trim().length > 0 : (status.data?.settings.paired ?? false)),
)

function settingsFromForm(): KoreaderSyncConfig {
  return {
    enabled,
    port: parsedPort,
    lan,
    username: username.trim() || null,
    // `null` keeps the stored password; an empty string clears it.
    password: passwordTouched ? password : null,
  }
}

const save = createMutation(() => ({
  mutationFn: (settings: KoreaderSyncConfig) => saveKoreaderSyncSettings(settings),
  onSuccess: (next) => {
    queryClient.setQueryData(koreaderSyncKeys.status(), next)
    enabled = next.settings.enabled
    port = String(next.settings.port)
    lan = next.settings.lan
    username = next.settings.username ?? ''
    password = next.settings.password ?? ''
    passwordTouched = false
    seeded = true
    toast.success(next.running ? 'KOReader sync listening' : 'KOReader sync off')
  },
  onError: (error: unknown) => {
    // Refused, so nothing changed: put the switch back rather than leave the
    // panel claiming a state the backend does not have.
    const previous = status.data?.settings.enabled ?? false
    enabled = previous
    if (switchControl) switchControl.checked = previous
    toast.error(koreaderSyncErrorMessage(error))
  },
}))

function onToggle(event: Event) {
  enabled = (event.currentTarget as WaSwitchElement).checked
}

function onLanToggle(event: Event) {
  lan = (event.currentTarget as WaSwitchElement).checked
}

function applySettings() {
  if (!portValid) {
    toast.error('Port must be between 1 and 65535')
    return
  }
  if (!credentialsValid) {
    toast.error('Set a username and password for KOReader to sign in with')
    return
  }
  save.mutate(settingsFromForm())
}

async function copyUrl(url: string) {
  try {
    await navigator.clipboard.writeText(url)
    toast.success('Sync URL copied')
  } catch {
    toast.error('Could not copy the sync URL')
  }
}
</script>

<section class="koreader" aria-labelledby="koreader-heading">
  <h2 id="koreader-heading">KOReader sync</h2>
  <p class="description">
    Keeps reading positions in step with KOReader's Progress sync plugin, so a
    book picked up on an e-reader continues on this machine — and the other way
    round. Every request is password-gated: enter the address, username, and
    password below in the plugin, on this reader only.
  </p>

  {#if status.isPending}
    <p class="muted">Loading…</p>
  {:else if status.isError}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      {koreaderSyncErrorMessage(status.error)}
    </wa-callout>
  {:else if !status.data}
    <!-- `invoke` can resolve `null`; reading through it here would throw during
         the Svelte flush and take every later effect with it. -->
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      The backend returned no KOReader sync settings.
    </wa-callout>
  {:else}
    <wa-callout variant={status.data.running ? 'success' : 'neutral'}>
      <wa-icon
        slot="icon"
        name={status.data.running ? 'circle-check' : 'circle-xmark'}
      ></wa-icon>
      {#if status.data.running && status.data.url}
        <span class="status">
          Sync server at <code>{status.data.url}</code>
          <ActionButton size="small" onclick={() => copyUrl(status.data.url ?? '')}>
            Copy
          </ActionButton>
        </span>
        <span class="hint">
          In KOReader: Tools → Progress sync → Custom sync server. Use this
          address, the username below, and the password you just set.
        </span>
      {:else}
        Not listening.
      {/if}
    </wa-callout>

    <div class="controls">
      <wa-switch
        id="koreader-sync-enabled"
        bind:this={switchControl}
        aria-label="Enable the KOReader sync server"
        checked={enabled}
        onchange={onToggle}
      >
        Enable KOReader sync
      </wa-switch>
      <wa-switch
        id="koreader-sync-lan"
        aria-label="Reach the sync server from other devices"
        checked={lan}
        onchange={onLanToggle}
      >
        Reachable from other devices
      </wa-switch>
      <wa-input
        id="koreader-sync-port"
        label="Port"
        type="number"
        min={1}
        max={65535}
        value={port}
        oninput={(event) => (port = event.currentTarget.value)}
      ></wa-input>
      <wa-input
        id="koreader-sync-username"
        label="Username"
        value={username}
        oninput={(event) => (username = event.currentTarget.value)}
      ></wa-input>
      <wa-input
        id="koreader-sync-password"
        label="Password"
        type="password"
        placeholder={status.data.settings.paired ? 'Stored — leave blank to keep' : 'Required'}
        value={password}
        oninput={(event) => {
          password = event.currentTarget.value
          passwordTouched = true
        }}
      ></wa-input>
      <ActionButton
        id="koreader-sync-save"
        onclick={applySettings}
        disabled={save.isPending || !portValid}
      >
        Save
      </ActionButton>
    </div>
    {#if !portValid}
      <p class="error">Port must be between 1 and 65535.</p>
    {:else if !credentialsValid}
      <p class="error">A username and password are required before KOReader can sign in.</p>
    {:else if !lan}
      <p class="hint">
        Bound to this machine only. Turn on "Reachable from other devices" for an
        e-reader on the same network.
      </p>
    {/if}
    <p class="hint">
      {status.data.books} book{status.data.books === 1 ? '' : 's'} mapped from this
      library. Books added since the server started are mapped on the next sync.
    </p>
  {/if}
</section>

<style>
  .koreader {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .koreader h2 {
    margin: 0;
  }

  .description {
    margin: 0;
    color: var(--wa-color-text-quiet);
  }

  .muted {
    margin: 0;
    color: var(--wa-color-text-quiet);
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

  .controls {
    display: flex;
    align-items: start;
    gap: var(--wa-space-m);
    flex-wrap: wrap;
    flex-direction: column;
  }

  .controls wa-input {
    width: 14rem;
  }

  .error {
    margin: 0;
    color: var(--wa-color-danger-fill-loud);
  }
</style>
