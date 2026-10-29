<script lang="ts">
import type { PluginSummary } from '../plugins'

let {
  plugin,
  open,
  onclose,
}: {
  plugin: PluginSummary | undefined
  open: boolean
  onclose: () => void
} = $props()

function signerLabel(plugin: PluginSummary): string {
  return plugin.signer.trim().length > 0 ? plugin.signer : 'unsigned'
}
</script>

<wa-drawer label="Plugin detail" placement="end" {open} onwa-after-hide={onclose}>
  {#if plugin}
  <div class="detail">
    <header class="head">
      <h2>{plugin.name}</h2>
      {#if plugin.version}
        <wa-badge variant="neutral">{plugin.version}</wa-badge>
      {/if}
    </header>

    <dl class="facts">
      <div class="fact">
        <dt class="label">Signer</dt>
        <dd class="value" class:unsigned={signerLabel(plugin) === 'unsigned'}>
          <wa-icon name="shield-halved"></wa-icon>
          {signerLabel(plugin)}
        </dd>
      </div>
      <div class="fact">
        <dt class="label">Status</dt>
        <dd class="value">{plugin.enabled ? 'Enabled' : 'Disabled'}</dd>
      </div>
      <div class="fact">
        <dt class="label">Installed at</dt>
        <dd class="value muted">plugins/{plugin.name}</dd>
      </div>
    </dl>

    <h3 class="section">Granted capabilities</h3>
    {#if plugin.granted.length > 0}
      <div class="caps">
        {#each plugin.granted as capability (capability)}
          <wa-tag size="small">{capability}</wa-tag>
        {/each}
      </div>
    {:else}
      <p class="muted">No capabilities granted.</p>
    {/if}
  </div>
  {/if}
</wa-drawer>

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .head h2 {
    margin: 0;
    font-size: var(--wa-font-size-xl);
    font-weight: var(--wa-font-weight-semibold);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    margin: 0;
    padding: 0;
  }

  .fact {
    display: flex;
    gap: var(--wa-space-s);
  }

  .label {
    min-width: 140px;
    margin: 0;
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
  }

  .value {
    margin: 0;
  }

  .value.unsigned {
    color: var(--wa-color-warning-60, var(--wa-color-text-quiet));
  }

  .value.muted {
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-s);
  }

  .section {
    margin: 0;
    font-size: var(--wa-font-size-s);
    font-weight: var(--wa-font-weight-semibold);
    color: var(--wa-color-text-quiet);
  }

  .caps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-2xs);
  }
</style>