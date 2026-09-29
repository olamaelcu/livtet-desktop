<script lang="ts">
import { createMutation, useQueryClient } from '@tanstack/svelte-query'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'svelte-sonner'
import type { RemotePlugin } from '../plugins'
import {
  addPluginFromPath,
  discoverRemotePlugins,
  installRemotePlugin,
  pluginErrorMessage,
} from '../plugins'
import { pluginKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

function invalidateInstalled() {
  void queryClient.invalidateQueries({ queryKey: pluginKeys.installed() })
}

const install = createMutation(() => ({
  mutationFn: (source: string) => addPluginFromPath(source),
  onSuccess: (plugin) => {
    toast.success(`Installed “${plugin.name}”`)
    invalidateInstalled()
  },
  onError: (error) => toast.error(pluginErrorMessage(error)),
}))

async function chooseFolder() {
  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: 'Select a plugin folder',
  })
  if (typeof selected === 'string') install.mutate(selected)
}

let registryUrl = $state('')
let remoteResults = $state<RemotePlugin[]>([])
let remoteError = $state<string | null>(null)

const browse = createMutation(() => ({
  mutationFn: (url: string) => discoverRemotePlugins(url),
  onSuccess: (results) => {
    remoteResults = results
    remoteError = null
  },
  onError: (error) => {
    remoteResults = []
    remoteError = pluginErrorMessage(error)
  },
}))

const installRemote = createMutation(() => ({
  mutationFn: ({ url, id }: { url: string; id: string }) => installRemotePlugin(url, id),
  onSuccess: (plugin) => {
    toast.success(`Installed “${plugin.name}”`)
    invalidateInstalled()
  },
  onError: (error) => toast.error(pluginErrorMessage(error)),
}))

function runBrowse() {
  const url = registryUrl.trim()
  if (url.length > 0) browse.mutate(url)
}

function submitBrowse(event: SubmitEvent) {
  event.preventDefault()
  runBrowse()
}
</script>

<wa-tab-group>
  <wa-tab slot="nav" panel="folder">From folder</wa-tab>
  <wa-tab slot="nav" panel="repository">From repository</wa-tab>

  <wa-tab-panel name="folder">
    <div class="panel">
      <p class="muted">
        Install a plugin from a local folder containing a <code>plugin.toml</code>.
        Its capabilities and signer are shown once installed.
      </p>
      <ActionButton variant="brand" disabled={install.isPending} onclick={chooseFolder}>
        {install.isPending ? 'Installing…' : 'Choose folder…'}
      </ActionButton>
    </div>
  </wa-tab-panel>

  <wa-tab-panel name="repository">
    <form class="panel" onsubmit={submitBrowse}>
      <p class="muted">Browse a plugin repository by its URL.</p>
      <div class="row">
        <wa-input
          placeholder="https://plugins.example.com"
          value={registryUrl}
          oninput={(event: Event) => {
            registryUrl = (event.target as HTMLInputElement).value
          }}
        ></wa-input>
        <ActionButton variant="brand" disabled={browse.isPending} onclick={runBrowse}>
          {browse.isPending ? 'Browsing…' : 'Browse'}
        </ActionButton>
      </div>

      {#if remoteError}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          {remoteError}
        </wa-callout>
      {:else if remoteResults.length > 0}
        <ul class="results">
          {#each remoteResults as plugin (plugin.id)}
            <li class="result">
              <div>
                <span class="name">{plugin.name}</span>
                {#if plugin.version}<wa-badge variant="neutral">{plugin.version}</wa-badge>{/if}
                {#if plugin.description}<p class="muted small">{plugin.description}</p>{/if}
              </div>
              <ActionButton
                disabled={installRemote.isPending}
                onclick={() => installRemote.mutate({ url: registryUrl.trim(), id: plugin.id })}
              >
                Install
              </ActionButton>
            </li>
          {/each}
        </ul>
      {/if}
    </form>
  </wa-tab-panel>
</wa-tab-group>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    padding-top: var(--wa-space-m);
  }

  .row {
    display: flex;
    gap: var(--wa-space-s);
    align-items: center;
  }

  .row wa-input {
    flex: 1;
  }

  .results {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .result {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
  }

  .name {
    font-weight: var(--wa-font-weight-semibold);
  }

  .muted {
    color: var(--wa-color-text-quiet);
  }

  .small {
    font-size: var(--wa-font-size-s);
    margin: var(--wa-space-2xs) 0 0;
  }
</style>
