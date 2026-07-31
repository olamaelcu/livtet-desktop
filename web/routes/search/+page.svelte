<script lang="ts">
import { onMount } from 'svelte'
import { on } from 'svelte/events'
import LocalCatalogSearch from '$lib/search/components/local-catalog-search.svelte'
import RemoteCatalogSearch from '$lib/search/components/remote-catalog-search.svelte'

let activeTab = $state('local')

onMount(() => {
  const group = document.querySelector('wa-tab-group')
  if (!group) return

  const off = on(group, 'wa-tab-show', (e: Event) => {
    const name = (e as CustomEvent).detail?.name
    if (name === 'local' || name === 'remote') activeTab = name
  })

  const activePanel = group.querySelector('wa-tab-panel[active]')
  if (activePanel) {
    const name = activePanel.getAttribute('name')
    if (name === 'local' || name === 'remote') activeTab = name
  }

  return off
})
</script>

<wa-tab-group>
  <wa-tab slot="nav" panel="local">Local Catalog</wa-tab>
  <wa-tab slot="nav" panel="remote">Remote Search</wa-tab>

  <wa-tab-panel name="local">
    {#if activeTab === "local"}
      <LocalCatalogSearch />
    {/if}
  </wa-tab-panel>
  <wa-tab-panel name="remote">
    {#if activeTab === "remote"}
      <RemoteCatalogSearch />
    {/if}
  </wa-tab-panel>
</wa-tab-group>
