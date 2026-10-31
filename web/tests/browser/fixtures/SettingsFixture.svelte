<script lang="ts">
import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query'
// Renders the real SettingsLayout, so the fixture cannot drift from the route.
// Only the panels are stand-ins (the real ones talk to Tauri). The wa-tab-*
// definitions are registered by the tests that import this fixture, because the
// scroll contract lives on wa-tab-group's own body part.
import AppFrame from '$lib/layout/AppFrame.svelte'
import SettingsLayout from '$routes/settings/SettingsLayout.svelte'

// AppFrame hosts the nav rail, which reads its badges through createQuery.
const client = new QueryClient({
  defaultOptions: {
    queries: { retry: false, gcTime: 0 },
    mutations: { retry: false },
  },
})
</script>

<QueryClientProvider {client}>
  <AppFrame>
    <SettingsLayout>
      <wa-tab-panel name="sync">
        <div style="height: 3000px">tall sync panel</div>
      </wa-tab-panel>
      <wa-tab-panel name="catalogs">
        <div style="height: 6000px">taller catalogs panel</div>
      </wa-tab-panel>
      <wa-tab-panel name="account">account</wa-tab-panel>
      <wa-tab-panel name="hotkeys">hotkeys</wa-tab-panel>
      <wa-tab-panel name="mcp">mcp</wa-tab-panel>
      <wa-tab-panel name="opds">opds</wa-tab-panel>
    </SettingsLayout>
  </AppFrame>
</QueryClientProvider>
