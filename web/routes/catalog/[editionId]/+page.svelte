<script lang="ts">
import { goto } from '$app/navigation'
import { page } from '$app/state'
import EditionDetail from '$lib/catalog/components/edition-detail.svelte'

interface Props {
  params: { editionId: string }
}

let { params }: Props = $props()

type TabId = 'overview' | 'files' | 'covers' | 'authors' | 'identifiers'

const ALLOWED_TABS: TabId[] = ['overview', 'files', 'covers', 'authors', 'identifiers']

const initialTab = $derived.by<TabId>(() => {
  const raw = page.url.searchParams.get('tab')
  if (raw && (ALLOWED_TABS as string[]).includes(raw)) {
    return raw as TabId
  }
  return 'overview'
})

function back(): void {
  void goto('/search')
}
</script>

<svelte:head>
  <title>Edition · livtet</title>
</svelte:head>

<wa-page>
  <header slot="header">
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <wa-button appearance="plain" role="button" tabindex="0" onclick={back}>
      <wa-icon slot="start" name="arrow-left"></wa-icon>
      Back to search
    </wa-button>
  </header>
  <main>
    <EditionDetail editionId={params.editionId} {initialTab} />
  </main>
</wa-page>

<style>
  main {
    padding: var(--wa-space-m, 1rem);
  }
</style>