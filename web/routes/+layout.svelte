<script lang="ts">
import '../app.css'
import '../app.wa.js'

import { createHotkey } from '@tanstack/svelte-hotkeys'
import { QueryClientProvider } from '@tanstack/svelte-query'
import { SvelteQueryDevtools } from '@tanstack/svelte-query-devtools'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'
import { listenToAtprotoEvents } from '../lib/atproto'
import CommandPalette from '../lib/components/CommandPalette.svelte'
import IncomingOfferDialog from '../lib/components/IncomingOfferDialog.svelte'
import TopLayerToaster from '../lib/components/TopLayerToaster.svelte'
import { bindingFor, initBindings } from '../lib/hotkeys/bindings.svelte'
import type { CommandId } from '../lib/hotkeys/commands'
import { focusSearchTarget } from '../lib/hotkeys/searchTarget.svelte'
import AppFrame from '../lib/layout/AppFrame.svelte'
import { listenToAppNavigation } from '../lib/navigation'
import { queryClient } from '../lib/query/client'
import { atprotoKeys } from '../lib/query/keys'
import { listenToSpotlightSelections } from '../lib/spotlight'
import { refreshLibraryOnPull } from '../lib/sync/library'

let { children } = $props()

const showDevtools =
  import.meta.env.DEV && import.meta.env.MODE !== 'test' && !import.meta.env.VITEST

let paletteOpen = $state(false)

function runCommand(id: CommandId) {
  switch (id) {
    case 'palette.open':
      paletteOpen = true
      break
    case 'search.focus':
      void focusSearchTarget()
      break
    case 'nav.library':
      void goto(resolve('/library'))
      break
    case 'nav.settings':
      void goto(resolve('/settings'))
      break
    case 'app.refresh':
      void queryClient.invalidateQueries()
      break
  }
}

createHotkey(
  () => bindingFor('palette.open'),
  () => {
    paletteOpen = true
  },
)
createHotkey(
  () => bindingFor('nav.library'),
  () => {
    void goto(resolve('/library'))
  },
)
createHotkey(
  () => bindingFor('nav.settings'),
  () => {
    void goto(resolve('/settings'))
  },
)
createHotkey(
  () => bindingFor('search.focus'),
  () => {
    void focusSearchTarget()
  },
)
createHotkey(
  () => bindingFor('app.refresh'),
  () => {
    void queryClient.invalidateQueries()
  },
)

onMount(() => {
  void initBindings()

  // The OAuth redirect finishes outside any command, so the result arrives as
  // an event. Subscribe here rather than in the Account panel: the callback
  // usually lands while the user is still in their browser, with no guarantee
  // that the settings tab is mounted.
  let unlisten: (() => void) | undefined
  let unlistenNavigation: (() => void) | undefined
  let unlistenSpotlight: (() => void) | undefined
  let unlistenLibraryPull: (() => void) | undefined
  let cancelled = false
  void listenToAppNavigation((route) => {
    void goto(route)
  })
    .then((stop) => {
      if (cancelled) stop()
      else unlistenNavigation = stop
    })
    // Outside Tauri (SSR, component tests) there is no event bridge.
    .catch(() => {})

  void listenToSpotlightSelections((editionId) => {
    const library = resolve('/library')
    void goto(`${library}?edition=${encodeURIComponent(editionId)}`)
  })
    .then((stop) => {
      if (cancelled) stop()
      else unlistenSpotlight = stop
    })
    .catch(() => {})

  // The daemon pulls on its own clock, so books can arrive with no click
  // behind them. Subscribing at the shell keeps every route's catalog views
  // live, not just an open Library tab.
  void refreshLibraryOnPull(queryClient)
    .then((stop) => {
      if (cancelled) stop()
      else unlistenLibraryPull = stop
    })
    // Outside Tauri (SSR, component tests) there is no event bridge.
    .catch(() => {})

  void listenToAtprotoEvents({
    onLogin: () => {
      toast.success('Signed in to ATProto')
      void queryClient.invalidateQueries({ queryKey: atprotoKeys.all })
    },
    onError: (message) => toast.error(message),
  })
    .then((stop) => {
      if (cancelled) stop()
      else unlisten = stop
    })
    // Outside Tauri (SSR, component tests) there is no event bridge; the
    // subscription is an enhancement, not a requirement.
    .catch(() => {})

  return () => {
    cancelled = true
    unlisten?.()
    unlistenNavigation?.()
    unlistenSpotlight?.()
    unlistenLibraryPull?.()
  }
})
</script>

<QueryClientProvider client={queryClient}>
  <AppFrame>
    {@render children?.()}
  </AppFrame>

  <TopLayerToaster />

  <!-- A paired device's offer is a decision the reader must see wherever
       they are, so the prompt lives beside the shell rather than in Settings. -->
  <IncomingOfferDialog />

  {#if paletteOpen}
    <CommandPalette onrun={runCommand} onclose={() => (paletteOpen = false)} />
  {/if}

  {#if showDevtools}
    <SvelteQueryDevtools buttonPosition="top-right"/>
  {/if}
</QueryClientProvider>
