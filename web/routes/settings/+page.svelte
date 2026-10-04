<script lang="ts">
import { page } from '$app/state'
import CatalogSettings from '../../lib/components/CatalogSettings.svelte'
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import SyncPanel from '../../lib/components/SyncPanel.svelte'
import NavRail from '../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../lib/layout/navItems'
import Pane from '../../lib/layout/Pane.svelte'
import RouteShell from '../../lib/layout/RouteShell.svelte'
</script>

<RouteShell>
  {#snippet nav()}
    <NavRail items={NAV_ITEMS} currentPath={page.url.pathname} expanded />
  {/snippet}
  <Pane>
<div class="settings">
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
</div>
  </Pane>
</RouteShell>

<style>
  .settings {
    padding: var(--wa-space-xl);
  }
</style>
