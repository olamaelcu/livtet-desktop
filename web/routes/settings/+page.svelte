<script lang="ts">
import AccountSettings from '../../lib/components/AccountSettings.svelte'
import CatalogSettings from '../../lib/components/CatalogSettings.svelte'
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import McpSettings from '../../lib/components/McpSettings.svelte'
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
  <wa-tab-panel name="account">
    <AccountSettings />
  </wa-tab-panel>
  <wa-tab-panel name="hotkeys">
    <HotkeySettings />
  </wa-tab-panel>
  <wa-tab-panel name="mcp">
    <McpSettings />
  </wa-tab-panel>
</SettingsLayout>
