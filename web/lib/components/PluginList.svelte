<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import ConfirmDialog from '../library/ConfirmDialog.svelte'
import type { PluginSummary } from '../plugins'
import { loadPlugins, pluginErrorMessage, removePlugin, setPluginEnabled } from '../plugins'
import { pluginKeys } from '../query/keys'
import ActionButton from './ActionButton.svelte'
import PluginDetailDrawer from './PluginDetailDrawer.svelte'

const queryClient = useQueryClient()

const plugins = createQuery(() => ({
  queryKey: pluginKeys.installed(),
  queryFn: loadPlugins,
}))

let pendingRemoval = $state<string | null>(null)
let detailTarget = $state<string | null>(null)

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

function without(map: Record<string, boolean>, name: string): Record<string, boolean> {
  const next = { ...map }
  delete next[name]
  return next
}

/**
 * Per-plugin overrides of the listed `enabled` state, so the switch and badge
 * follow the user's intent while the command is in flight. An override is
 * dropped once the refetched list carries the new state, and put back to what
 * the backend still has if the command is refused.
 */
let intent = $state<Record<string, boolean>>({})
let toggling = $state<Record<string, boolean>>({})

function isEnabled(plugin: PluginSummary): boolean {
  return intent[plugin.name] ?? plugin.enabled
}

async function toggle(plugin: PluginSummary, control: WaSwitchElement) {
  const previous = isEnabled(plugin)
  const next = control.checked
  if (next === previous) return

  intent = { ...intent, [plugin.name]: next }
  toggling = { ...toggling, [plugin.name]: true }
  try {
    await setPluginEnabled(plugin.name, next)
    await queryClient.invalidateQueries({ queryKey: pluginKeys.installed() })
    intent = without(intent, plugin.name)
  } catch (error) {
    intent = { ...intent, [plugin.name]: previous }
    control.checked = previous
    toast.error(pluginErrorMessage(error))
  } finally {
    toggling = without(toggling, plugin.name)
  }
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
        {@const enabled = isEnabled(plugin)}
        <li class="card" class:disabled={!enabled} data-enabled={enabled}>
          <div class="head">
            <div class="title">
              <span class="name">{plugin.name}</span>
              {#if plugin.version}<wa-badge variant="neutral">{plugin.version}</wa-badge>{/if}
              <wa-badge class="status" variant={enabled ? 'success' : 'neutral'}
                >{enabled ? 'enabled' : 'disabled'}</wa-badge
              >
            </div>
            <div class="actions">
              <wa-switch
                aria-label={`Enable ${plugin.name}`}
                checked={enabled}
                disabled={toggling[plugin.name] ?? false}
                onchange={(event) => {
                  void toggle(plugin, event.target as WaSwitchElement)
                }}
              ></wa-switch>
              <ActionButton
          disabled={remove.isPending && pendingRemoval === plugin.name}
                onclick={() => {
                  pendingRemoval = plugin.name
                }}
              >
                Remove
              </ActionButton>
              <ActionButton
                aria-haspopup="dialog"
                aria-expanded={detailTarget === plugin.name}
                onclick={() => {
                  detailTarget = plugin.name
                }}
              >
                <wa-icon slot="icon" name="circle-info"></wa-icon>
                Info
              </ActionButton>
            </div>
          </div>
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

<PluginDetailDrawer
  plugin={plugins.data?.find(p => p.name === detailTarget) as PluginSummary}
  open={detailTarget !== null}
  onclose={() => { detailTarget = null }}
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

  .card.disabled {
    border-style: dashed;
  }

  .card.disabled .name {
    opacity: 0.6;
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
    flex-wrap: wrap;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
  }

  .name {
    font-weight: var(--wa-font-weight-semibold);
  }
</style>
