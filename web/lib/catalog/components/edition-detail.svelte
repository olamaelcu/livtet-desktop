<script lang="ts">
  import EditionTabOverview from "./edition-tab-overview.svelte";
  import EditionTabFiles from "./edition-tab-files.svelte";
  import EditionTabAuthors from "./edition-tab-authors.svelte";
  import EditionTabIdentifiers from "./edition-tab-identifiers.svelte";

  type TabId = "overview" | "files" | "authors" | "identifiers";

  interface Props {
    editionId: string;
    /**
     * When set, jumps the active tab on mount and whenever it
     * changes. Used by the dedicated `/catalog/[editionId]` route to
     * honour `?tab=`; the peek dialog leaves it `undefined` and
     * always starts on Overview.
     */
    initialTab?: TabId;
  }

  let { editionId, initialTab }: Props = $props();

  // `active` defaults to Overview and is synced from `initialTab` via
  // the effect below on first render and on every change. We
  // intentionally do NOT use `initialTab` in the $state initializer
  // because Svelte flags that pattern as a stale-value trap.
  let active = $state<TabId>("overview");

  // If `initialTab` changes after mount (e.g. user navigates from
  // ?tab=files to ?tab=authors on the route), keep the tabs in sync.
  $effect(() => {
    if (initialTab && initialTab !== active) active = initialTab;
  });

  function onTabShow(event: CustomEvent<{ name: string }>): void {
    const next = event.detail.name;
    if (
      next === "overview" ||
      next === "files" ||
      next === "authors" ||
      next === "identifiers"
    ) {
      active = next;
    }
  }
</script>

<wa-tab-group onwa-tab-show={onTabShow}>
  <wa-tab panel="overview" active={active === "overview"}>Overview</wa-tab>
  <wa-tab panel="files" active={active === "files"}>Files</wa-tab>
  <wa-tab panel="authors" active={active === "authors"}>Authors</wa-tab>
  <wa-tab panel="identifiers" active={active === "identifiers"}>
    Identifiers
  </wa-tab>

  <wa-tab-panel name="overview" active={active === "overview"}>
    <EditionTabOverview {editionId} />
  </wa-tab-panel>
  <wa-tab-panel name="files" active={active === "files"}>
    <EditionTabFiles {editionId} />
  </wa-tab-panel>
  <wa-tab-panel name="authors" active={active === "authors"}>
    <EditionTabAuthors {editionId} />
  </wa-tab-panel>
  <wa-tab-panel name="identifiers" active={active === "identifiers"}>
    <EditionTabIdentifiers {editionId} />
  </wa-tab-panel>
</wa-tab-group>

<style>
  wa-tab-panel {
    padding: var(--wa-space-m, 1rem) 0;
  }
</style>