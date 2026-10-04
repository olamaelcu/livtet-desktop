<script lang="ts">
import CatalogSettings from '../../lib/components/CatalogSettings.svelte'
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import SyncPanel from '../../lib/components/SyncPanel.svelte'
import SettingsLayout from './SettingsLayout.svelte'
</script>

<SettingsLayout>
  <wa-tab-panel name="sync">
    <SyncPanel />
  </wa-tab-panel>
  <wa-tab-panel name="catalogs">
    <CatalogSettings />
  </wa-tab-panel>
  <wa-tab-panel name="hotkeys">
    <HotkeySettings />
  </wa-tab-panel>
</SettingsLayout>
