<script lang="ts">
import '../app.css'
import '../app.wa.js'

import { createHotkey } from '@tanstack/svelte-hotkeys'
import { QueryClientProvider } from '@tanstack/svelte-query'
import { SvelteQueryDevtools } from '@tanstack/svelte-query-devtools'
import { onMount, tick } from 'svelte'
import { Toaster } from 'svelte-sonner'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'
import { page } from '$app/state'

import CommandPalette from '../lib/components/CommandPalette.svelte'
import { bindingFor, initBindings } from '../lib/hotkeys/bindings.svelte'

import type { CommandId } from '../lib/hotkeys/commands'

import { queryClient } from '../lib/query/client'

let { children } = $props()

let pageTitle = $derived(page.data.pageTitle ?? 'Livtet')
const showDevtools = import.meta.env.DEV

let paletteOpen = $state(false)

const navItems = [
  { href: '/library', label: 'Library' },
  { href: '/catalog', label: 'Catalogs' },
  { href: '/plugins', label: 'Plugins' },
  { href: '/settings', label: 'Settings' },
] as const

function isActive(href: string) {
  return page.url.pathname === href || page.url.pathname.startsWith(`${href}/`)
}

async function focusSearch() {
  if (!document.getElementById('library-search')) {
    await goto(resolve('/library'))
    await tick()
  }
  document.getElementById('library-search')?.focus()
}

function runCommand(id: CommandId) {
  switch (id) {
    case 'palette.open':
      paletteOpen = true
      break
    case 'search.focus':
      void focusSearch()
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
    void focusSearch()
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
  <wa-page mobile-breakpoint="960">
    <header slot="header">
      <wa-button appearance="plain" data-toggle-nav aria-label="Toggle navigation">
        <wa-icon name="bars"></wa-icon>
      </wa-button>
      <span class="page-title">{pageTitle}</span>
      <wa-button-group>
        <wa-button href="/" variant="brand">
          <wa-icon name="home"></wa-icon>
          Livtet
        </wa-button>
        <wa-button href="/">
          <wa-icon name="circle-question"></wa-icon>
          Help
        </wa-button>
      </wa-button-group>
    </header>
    <nav slot="navigation" aria-label="Primary">
      <wa-button-group orientation="vertical" label="Navigation">
        {#each navItems as item (item.href)}
          <wa-button
            href={item.href}
            variant={isActive(item.href) ? 'brand' : 'neutral'}
            appearance={isActive(item.href) ? 'filled' : 'plain'}
          >
            {item.label}
          </wa-button>
        {/each}
      </wa-button-group>
    </nav>
    {@render children?.()}
    <footer slot="footer">
      <small>Livtet</small>
    </footer>
  </wa-page>

  <Toaster theme="system" position="bottom-left" />

  {#if paletteOpen}
    <CommandPalette onrun={runCommand} onclose={() => (paletteOpen = false)} />
  {/if}

  {#if showDevtools}
    <SvelteQueryDevtools />
  {/if}
</QueryClientProvider>

<style>
  nav {
    padding-top: var(--wa-space-xs);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);

    > .page-title {
      flex: 1 1 auto;
    }
  }
  footer {
    padding: var(--wa-space-s) var(--wa-space-m);
    text-align: center;
    color: var(--wa-color-neutral-on-quiet);
  }
  /* The nav toggle only makes sense while the menu is a drawer. */
  :global(wa-page[view='desktop']) [data-toggle-nav] {
    display: none;
  }
</style>
