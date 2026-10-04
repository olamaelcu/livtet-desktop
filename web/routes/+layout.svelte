<script lang="ts">
import '../app.css'
import '../app.wa.js'

import { createHotkey } from '@tanstack/svelte-hotkeys'
import { QueryClientProvider } from '@tanstack/svelte-query'
import { SvelteQueryDevtools } from '@tanstack/svelte-query-devtools'
import { onMount } from 'svelte'
import { Toaster } from 'svelte-sonner'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'
import CommandPalette from '../lib/components/CommandPalette.svelte'
import { bindingFor, initBindings } from '../lib/hotkeys/bindings.svelte'
import type { CommandId } from '../lib/hotkeys/commands'
import { focusSearchTarget } from '../lib/hotkeys/searchTarget.svelte'
import AppFrame from '../lib/layout/AppFrame.svelte'

import { queryClient } from '../lib/query/client'

let { children } = $props()

const showDevtools = import.meta.env.DEV

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
})
</script>

<QueryClientProvider client={queryClient}>
  <AppFrame>
    {@render children?.()}
  </AppFrame>

  <Toaster theme="system" position="bottom-left" />

  {#if paletteOpen}
    <CommandPalette onrun={runCommand} onclose={() => (paletteOpen = false)} />
  {/if}

  {#if showDevtools}
    <SvelteQueryDevtools />
  {/if}
</QueryClientProvider>
