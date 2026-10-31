<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import type { OpdsServerSettings as OpdsServerConfig } from '../bindings'
import { getOpdsServerStatus, opdsServerErrorMessage, saveOpdsServerSettings } from '../opds-server'
import { opdsServerKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const status = createQuery(() => ({
  queryKey: opdsServerKeys.status(),
  queryFn: getOpdsServerStatus,
}))

let enabled = $state(false)
let port = $state('8080')
let switchControl = $state<WaSwitchElement>()

// Plain (not `$state`) so it is not an effect dependency: the form takes its
// initial values from the backend once, then belongs to the user until Save.
let seeded = false
$effect(() => {
  const current = status.data
  if (seeded || !current) return
  enabled = current.settings.enabled
  port = String(current.settings.port)
  seeded = true
})

const parsedPort = $derived(Number(port))
const portValid = $derived(Number.isInteger(parsedPort) && parsedPort >= 1 && parsedPort <= 65535)

const save = createMutation(() => ({
  mutationFn: (settings: OpdsServerConfig) => saveOpdsServerSettings(settings),
  onSuccess: (next) => {
    queryClient.setQueryData(opdsServerKeys.status(), next)
    enabled = next.settings.enabled
    port = String(next.settings.port)
    seeded = true
    toast.success(next.running ? 'OPDS server listening' : 'OPDS server off')
  },
  onError: (error: unknown) => {
    // Refused, so nothing changed: put the switch back rather than leave the
    // panel claiming a state the backend does not have. wa-switch owns its own
    // `checked` once clicked, so resetting the data behind it is not enough.
    const previous = status.data?.settings.enabled ?? false
    enabled = previous
    if (switchControl) switchControl.checked = previous
    toast.error(opdsServerErrorMessage(error))
  },
}))

function onToggle(event: Event) {
  enabled = (event.currentTarget as WaSwitchElement).checked
}

function applySettings() {
  if (!portValid) {
    toast.error('Port must be between 1 and 65535')
    return
  }
  save.mutate({ enabled, port: parsedPort })
}

async function copyUrl(url: string) {
  try {
    await navigator.clipboard.writeText(url)
    toast.success('OPDS URL copied')
  } catch {
    toast.error('Could not copy the OPDS URL')
  }
}
</script>

<section class="opds" aria-labelledby="opds-heading">
  <h2 id="opds-heading">OPDS server</h2>
  <p class="description">
    Publishes the books stored on this machine as an OPDS 2.0 catalog, so an
    e-reader on the same network can browse and download them. It listens on
    every network interface and has no password: anyone who can reach the port
    can read the library, so turn it off when you are on a network you do not
    trust.
  </p>

  {#if status.isPending}
    <p class="muted">Loading…</p>
  {:else if status.isError}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      {opdsServerErrorMessage(status.error)}
    </wa-callout>
  {:else}
    <wa-callout variant={status.data.running ? 'success' : 'neutral'}>
      <wa-icon
        slot="icon"
        name={status.data.running ? 'circle-check' : 'circle-xmark'}
      ></wa-icon>
      {#if status.data.running && status.data.url}
        <span class="status">
          Catalog at <code>{status.data.url}</code>
          <ActionButton size="small" onclick={() => copyUrl(status.data.url ?? '')}>
            Copy
          </ActionButton>
        </span>
        <span class="hint">
          Add that address to your e-reader. Another device on the network uses
          this machine's address with the same port.
        </span>
      {:else}
        Not listening.
      {/if}
    </wa-callout>

    <div class="controls">
      <wa-switch
        bind:this={switchControl}
        aria-label="Enable the OPDS server"
        checked={enabled}
        onchange={onToggle}
      >
        Enable the OPDS server
      </wa-switch>
      <wa-input
        label="Port"
        type="number"
        min={1}
        max={65535}
        value={port}
        disabled={!enabled}
        oninput={(event) => (port = event.currentTarget.value)}
      ></wa-input>
      <ActionButton onclick={applySettings} disabled={save.isPending || !portValid}>
        Save
      </ActionButton>
    </div>
    {#if !portValid}
      <p class="error">Port must be between 1 and 65535.</p>
    {/if}
  {/if}
</section>

<style>
  .opds {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .opds h2 {
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
    align-items: flex-end;
    gap: var(--wa-space-m);
    flex-wrap: wrap;
  }

  .controls wa-input {
    width: 10rem;
  }

  .error {
    margin: 0;
    color: var(--wa-color-danger-fill-loud);
  }
</style>
