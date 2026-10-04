<script lang="ts">
// Mirrors the wrapper structure of web/routes/settings/+page.svelte. The real
// panels talk to Tauri, so the tab content here is a stand-in that is simply
// taller than the frame -- which is all the scroll-region contract cares about.
//
// The wa-tab-* elements are deliberately left unregistered (this fixture's test
// does not import their definitions), so the stand-in content lays out directly
// and the assertions measure the route's wrapper chain rather than
// wa-tab-group's internals, which Task 4 changes anyway. That is sound here
// because wa-tab-group contributes no vertical scroller of its own: its body
// part computes overflow-y: visible.
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import NavRail from '../../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../../lib/layout/navItems'
import Pane from '../../../lib/layout/Pane.svelte'
import RouteShell from '../../../lib/layout/RouteShell.svelte'
import ScrollRegion from '../../../lib/layout/ScrollRegion.svelte'
</script>

<AppFrame>
  <RouteShell>
    {#snippet nav()}
      <NavRail items={NAV_ITEMS} currentPath="/settings" expanded />
    {/snippet}
    <Pane>
      <ScrollRegion>
        <div class="settings">
          <wa-tab-group>
            <wa-tab slot="nav" panel="sync">Sync</wa-tab>
            <wa-tab slot="nav" panel="catalogs">Catalogs</wa-tab>
            <wa-tab slot="nav" panel="hotkeys">Keyboard</wa-tab>

            <wa-tab-panel name="sync">
              <div style="height: 3000px">tall sync panel</div>
            </wa-tab-panel>
            <wa-tab-panel name="catalogs">catalogs</wa-tab-panel>
            <wa-tab-panel name="hotkeys">hotkeys</wa-tab-panel>
          </wa-tab-group>
        </div>
      </ScrollRegion>
    </Pane>
  </RouteShell>
</AppFrame>

<style>
  .settings {
    padding: var(--wa-space-xl);
  }
</style>
