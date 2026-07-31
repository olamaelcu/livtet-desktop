<script lang="ts">
  import { onMount } from "svelte";
  import LocalCatalogSearch from "$lib/search/components/local-catalog-search.svelte";
  import RemoteCatalogSearch from "$lib/search/components/remote-catalog-search.svelte";

  let activeTab = $state("local");

  onMount(() => {
    function onShow(e: Event) {
      const tab = e.target as HTMLElement;
      const panel = tab.getAttribute("panel");
      if (panel === "local") activeTab = "local";
      if (panel === "remote") activeTab = "remote";
    }
    document.addEventListener("wa-show", onShow);
    return () => document.removeEventListener("wa-show", onShow);
  });
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
