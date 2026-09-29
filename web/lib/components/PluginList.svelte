<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import ConfirmDialog from '../library/ConfirmDialog.svelte'
import type { PluginSummary } from '../plugins'
import { loadPlugins, pluginErrorMessage, removePlugin } from '../plugins'
import { pluginKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'

const queryClient = useQueryClient()

const plugins = createQuery(() => ({
  queryKey: pluginKeys.installed(),
  queryFn: loadPlugins,
}))

let pendingRemoval = $state<string | null>(null)

const remove = createMutation(() => ({
  mutationFn: (name: string) => removePlugin(name),
  onSuccess: (_data, name) => {
    toast.success(`Removed “${name}”`)
    void queryClient.invalidateQueries({ queryKey: pluginKeys.installed() })
  },
  onError: (error) => toast.error(pluginErrorMessage(error)),
  onSettled: () => {
    pendingRemoval = null
  },
}))

function signerLabel(plugin: PluginSummary): string {
  return plugin.signer.trim().length > 0 ? plugin.signer : 'unsigned'
}
</script>

<section class="plugins">
  {#if plugins.isPending}
    <p class="muted">Loading plugins…</p>
  {:else if plugins.isError}
    <wa-callout variant="danger">
      <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
      {pluginErrorMessage(plugins.error)}
    </wa-callout>
  {:else if (plugins.data ?? []).length === 0}
    <p class="muted">
      No plugins installed. Add one from a local folder or a repository above.
    </p>
  {:else}
    <ul class="list">
      {#each plugins.data ?? [] as plugin (plugin.name)}
        <li class="card">
          <div class="head">
            <div class="title">
              <span class="name">{plugin.name}</span>
              {#if plugin.version}<wa-badge variant="neutral">{plugin.version}</wa-badge>{/if}
            </div>
            <ActionButton
              disabled={remove.isPending && pendingRemoval === plugin.name}
              onclick={() => {
                pendingRemoval = plugin.name
              }}
            >
              Remove
            </ActionButton>
          </div>
          <div class="signer" class:unsigned={signerLabel(plugin) === 'unsigned'}>
            <wa-icon name="shield-halved"></wa-icon>
            {signerLabel(plugin)}
          </div>
          {#if plugin.granted.length > 0}
            <div class="caps">
              {#each plugin.granted as capability (capability)}
                <wa-tag size="small">{capability}</wa-tag>
              {/each}
            </div>
          {:else}
            <p class="muted small">No capabilities granted.</p>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<ConfirmDialog
  open={pendingRemoval !== null}
  title="Remove plugin"
  message={`Remove “${pendingRemoval ?? ''}”? This deletes the plugin from your library and revokes its access.`}
  confirmLabel="Remove"
  onconfirm={() => {
    if (pendingRemoval) remove.mutate(pendingRemoval)
  }}
  oncancel={() => {
    pendingRemoval = null
  }}
/>

<style>
  .plugins {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    padding: var(--wa-space-m);
    border: 1px solid var(--wa-color-border-default);
    border-radius: var(--wa-radius-m);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .name {
    font-weight: var(--wa-font-weight-semibold);
  }

  .signer {
    display: flex;
    align-items: center;
    gap: var(--wa-space-2xs);
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
  }

  .signer.unsigned {
    color: var(--wa-color-warning-60, var(--wa-color-text-quiet));
  }

  .caps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-2xs);
  }

  .muted {
    color: var(--wa-color-text-quiet);
  }

  .small {
    font-size: var(--wa-font-size-s);
  }
</style>
