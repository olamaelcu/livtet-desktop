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
import TopLayerToaster from '../lib/components/TopLayerToaster.svelte'
import { bindingFor, initBindings } from '../lib/hotkeys/bindings.svelte'
import type { CommandId } from '../lib/hotkeys/commands'
import { focusSearchTarget } from '../lib/hotkeys/searchTarget.svelte'
import AppFrame from '../lib/layout/AppFrame.svelte'

import { queryClient } from '../lib/query/client'
import { atprotoKeys } from '../lib/query/keys'

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
  let cancelled = false
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
  }
})
</script>

<QueryClientProvider client={queryClient}>
  <AppFrame>
    {@render children?.()}
  </AppFrame>

  <TopLayerToaster />

  {#if paletteOpen}
    <CommandPalette onrun={runCommand} onclose={() => (paletteOpen = false)} />
  {/if}

  {#if showDevtools}
    <SvelteQueryDevtools buttonPosition="top-right"/>
  {/if}
</QueryClientProvider>
