<script lang="ts">
import '@awesome.me/webawesome/dist/styles/webawesome.css'
import '../app.wa.js'
import '../app.css'

import { setHotkeysContext } from '@tanstack/svelte-hotkeys'
import { Toaster } from 'svelte-sonner'
import { commands } from '$lib/bindings'
import PeekDialog from '$lib/catalog/components/peek-dialog.svelte'
import AlreadyInCatalogDialog from '$lib/catalog/components/already-in-catalog-dialog.svelte'
import { defaultCommands, initHotkeyBridge, useCommandRegistry } from '$lib/commands'
import CommandPalette from '$lib/commands/components/command-palette.svelte'
import CommandReconciler from '$lib/commands/components/command-reconciler.svelte'
import HeldKeysIndicator from '$lib/commands/components/held-keys-indicator.svelte'
import HelpOverlay from '$lib/commands/components/help-overlay.svelte'
import { subscribeProviderFailures } from '$lib/remote/chain'

let { children } = $props()

setHotkeysContext({
  hotkey: {
    preventDefault: true,
    stopPropagation: true,
    ignoreInputs: true,
  },
  hotkeySequence: {
    timeout: 1500,
  },
})

const bridge = initHotkeyBridge()
const registry = useCommandRegistry()

// Register the default command set once at startup.
for (const c of defaultCommands) registry.register(c)

subscribeProviderFailures()
</script>

<CommandReconciler customProfile={bridge.getCustomProfile()} />

<Toaster position="bottom-right" richColors closeButton />

<wa-page>
  <wa-button slot="navigation-header" href="/">Home</wa-button>
  <nav slot="navigation">
    <wa-button-group class="nav">
      <wa-button href="/search">Catalog</wa-button>
      <wa-button href="/settings">Settings</wa-button>
    </wa-button-group>
  </nav>
  <main>
    {@render children()}
  </main>
</wa-page>

<CommandPalette {bridge} />
<HelpOverlay />
<HeldKeysIndicator />
<PeekDialog />
<AlreadyInCatalogDialog />

<style>
  main {
    padding: 0;
  }

  .nav {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs);
  }
</style>
