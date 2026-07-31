<script lang="ts">
  import { onMount } from "svelte";
  import LocalCatalogSearch from "$lib/search/components/local-catalog-search.svelte";
  import RemoteCatalogSearch from "$lib/search/components/remote-catalog-search.svelte";

  let activeTab = $state("local");

  onMount(() => {
    const observer = new MutationObserver((mutations) => {
      for (const m of mutations) {
        if (m.type === "attributes" && m.attributeName === "active") {
          const panel = m.target as HTMLElement;
          if (panel.hasAttribute("active")) {
            const name = panel.getAttribute("name");
            if (name === "local" || name === "remote") activeTab = name;
          }
        }
      }
    });

    const panels = document.querySelectorAll("wa-tab-panel");
    panels.forEach((p) => observer.observe(p, { attributes: true, attributeFilter: ["active"] }));

    // Sync initial state — WA may have activated a tab before observer was set up
    for (const p of panels) {
      if (p.hasAttribute("active")) {
        const name = p.getAttribute("name");
        if (name === "local" || name === "remote") activeTab = name;
        break;
      }
    }

    return () => observer.disconnect();
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
