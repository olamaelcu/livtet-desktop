<script lang="ts">
import AccountSettings from '../../lib/components/AccountSettings.svelte'
import CatalogSettings from '../../lib/components/CatalogSettings.svelte'
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import McpSettings from '../../lib/components/McpSettings.svelte'
import OpdsServerSettings from '../../lib/components/OpdsServerSettings.svelte'
import SettingsLayout from './SettingsLayout.svelte'
</script>

<SettingsLayout>
  <!--
    One OPDS tab, two nested panes: where the catalog records come from, and
    this app's own OPDS server. They are read side by side often enough that
    splitting them across top-level tabs made the strip longer than the work.
  -->
  <wa-tab-panel name="opds">
    <wa-tab-group class="opds-tabs">
      <wa-tab slot="nav" panel="catalogs">
        <wa-icon name="list"></wa-icon>
        Catalogs
      </wa-tab>
      <wa-tab slot="nav" panel="server">
        <wa-icon name="server"></wa-icon>
        OPDS server
      </wa-tab>
      <wa-tab-panel name="catalogs">
        <CatalogSettings />
      </wa-tab-panel>
      <wa-tab-panel name="server">
        <OpdsServerSettings />
      </wa-tab-panel>
    </wa-tab-group>
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

<style>
  /* The nested group sits inside the outer body, which already scrolls and
     pads: it lays out as a block and adds no padding of its own. */
  .opds-tabs {
    display: block;
    min-height: 0;
  }

  .opds-tabs::part(base) {
    display: block;
    min-height: 0;
  }

  .opds-tabs::part(body) {
    padding: 0;
  }

  .opds-tabs::part(nav) {
    justify-content: flex-start;
    padding-inline: 0;
  }

  .opds-tabs wa-tab wa-icon {
    margin-inline-end: var(--wa-space-2xs);
  }
</style>
