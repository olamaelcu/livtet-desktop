<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import type { McpSettings as McpSettingsConfig } from '../bindings'
import { getMcpStatus, mcpErrorMessage, saveMcpSettings } from '../mcp'
import { mcpKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const status = createQuery(() => ({
  queryKey: mcpKeys.status(),
  queryFn: getMcpStatus,
}))

let enabled = $state(false)
let port = $state('7777')
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
  mutationFn: (settings: McpSettingsConfig) => saveMcpSettings(settings),
  onSuccess: (next) => {
    queryClient.setQueryData(mcpKeys.status(), next)
    enabled = next.settings.enabled
    port = String(next.settings.port)
    seeded = true
    toast.success(next.running ? 'MCP server listening' : 'MCP server off')
  },
  onError: (error: unknown) => {
    // Refused, so nothing changed: put the switch back rather than leave the
    // panel claiming a state the backend does not have. wa-switch owns its own
    // `checked` once clicked, so resetting the data behind it is not enough.
    const previous = status.data?.settings.enabled ?? false
    enabled = previous
    if (switchControl) switchControl.checked = previous
    toast.error(mcpErrorMessage(error))
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
    toast.success('MCP URL copied')
  } catch {
    toast.error('Could not copy the MCP URL')
  }
}
</script>

<section class="mcp" aria-labelledby="mcp-heading">
  <h2 id="mcp-heading">MCP server</h2>
  <p class="description">
    Lets a local assistant or script search the library, import books, and read
    or bookmark them over the Model Context Protocol. The server listens on
    loopback only (<code>127.0.0.1</code>) and needs no token; add the URL to
    your client's <code>mcpServers</code> configuration.
  </p>

  {#if status.isPending}
    <p class="muted">Loading…</p>
  {:else if status.isError}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      {mcpErrorMessage(status.error)}
    </wa-callout>
  {:else if !status.data}
    <!-- `invoke` can resolve `null`; reading through it here would throw during
         the Svelte flush and take every later effect with it. -->
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      The backend returned no MCP settings.
    </wa-callout>
  {:else}
    {#if status.data.env_port !== null}
      <wa-callout variant="warning">
        <wa-icon slot="icon" name="circle-info"></wa-icon>
        <code>LIVTET_MCP_PORT</code> is set for this launch, so the server listens on port
        {status.data.env_port} regardless of what is saved here.
      </wa-callout>
    {/if}

    <wa-callout variant={status.data.running ? 'success' : 'neutral'}>
      <wa-icon
        slot="icon"
        name={status.data.running ? 'circle-check' : 'circle-xmark'}
      ></wa-icon>
      {#if status.data.running && status.data.url}
        <span class="status">
          Listening at <code>{status.data.url}</code>
          <ActionButton size="small" onclick={() => copyUrl(status.data.url ?? '')}>
            Copy
          </ActionButton>
        </span>
      {:else}
        Not listening.
      {/if}
    </wa-callout>

    <div class="controls">
      <wa-switch
        bind:this={switchControl}
        aria-label="Enable the MCP server"
        checked={enabled}
        onchange={onToggle}
      >
        Enable the MCP server
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
  .mcp {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .mcp h2 {
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
