<script lang="ts">
  import LocalCatalogSearch from '$lib/search/components/local-catalog-search.svelte'
  import RemoteCatalogSearch from '$lib/search/components/remote-catalog-search.svelte'

  let selectedTab = $state("local")
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<wa-tab-group>
  <wa-tab
    slot="nav"
    panel="local"
    active={selectedTab === "local"}
    onclick={() => (selectedTab = "local")}
  >
    Local Catalog
  </wa-tab>
  <wa-tab
    slot="nav"
    panel="remote"
    active={selectedTab === "remote"}
    onclick={() => (selectedTab = "remote")}
  >
    Remote Search
  </wa-tab>

  <wa-tab-panel name="local">
    {#if selectedTab === "local"}
      <LocalCatalogSearch />
    {/if}
  </wa-tab-panel>
  <wa-tab-panel name="remote">
    {#if selectedTab === "remote"}
      <RemoteCatalogSearch />
    {/if}
  </wa-tab-panel>
</wa-tab-group>
