<script lang="ts">
import { untrack } from 'svelte'
import EditionTabAuthors from './edition-tab-authors.svelte'
import EditionTabCovers from './edition-tab-covers.svelte'
import EditionTabFiles from './edition-tab-files.svelte'
import EditionTabIdentifiers from './edition-tab-identifiers.svelte'
import EditionTabOverview from './edition-tab-overview.svelte'

type TabId = 'overview' | 'files' | 'covers' | 'authors' | 'identifiers'

interface Props {
  editionId: string
  /**
   * When set, jumps the active tab on mount and whenever it
   * changes. Used by the dedicated `/catalog/[editionId]` route to
   * honour `?tab=`; the peek dialog leaves it `undefined` and
   * always starts on Overview.
   */
  initialTab?: TabId
}

let { editionId, initialTab }: Props = $props()

// `active` defaults to Overview and is synced from `initialTab` via
// the effect below on first render and on every change. We
// intentionally do NOT use `initialTab` in the $state initializer
// because Svelte flags that pattern as a stale-value trap.
let active = $state<TabId>('overview')

// Sync active tab when `initialTab` changes (e.g. URL ?tab= query
// param changes). `untrack` keeps this effect from reacting to
// user-initiated tab clicks — without it the effect fights every
// manual tab change and snaps back to whatever `initialTab` says.
$effect(() => {
  if (initialTab && initialTab !== untrack(() => active)) active = initialTab
})

function onTabShow(event: CustomEvent<{ name: string }>): void {
  active = event.detail.name as TabId
}
</script>

<wa-tab-group onwa-tab-show={onTabShow}>
  <wa-tab panel="overview" active={active === "overview"}>Overview</wa-tab>
  <wa-tab panel="files" active={active === "files"}>Files</wa-tab>
  <wa-tab panel="covers" active={active === "covers"}>Covers</wa-tab>
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
  <wa-tab-panel name="covers" active={active === "covers"}>
    <EditionTabCovers {editionId} />
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

