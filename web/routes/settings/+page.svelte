<script lang="ts">
import CatalogSettings from '../../lib/components/CatalogSettings.svelte'
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import SyncPanel from '../../lib/components/SyncPanel.svelte'
</script>

<main class="settings">
  <wa-tab-group>
    <wa-tab slot="nav" panel="sync">Sync</wa-tab>
    <wa-tab slot="nav" panel="catalogs">Catalogs</wa-tab>
    <wa-tab slot="nav" panel="hotkeys">Keyboard</wa-tab>

    <wa-tab-panel name="sync">
      <SyncPanel />
    </wa-tab-panel>
    <wa-tab-panel name="catalogs">
      <CatalogSettings />
    </wa-tab-panel>
    <wa-tab-panel name="hotkeys">
      <HotkeySettings />
    </wa-tab-panel>
  </wa-tab-group>
</main>

<style>
  .settings {
    padding: var(--wa-space-xl);
  }
</style>
