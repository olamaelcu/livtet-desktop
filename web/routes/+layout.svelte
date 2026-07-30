<script lang="ts">
  import "@awesome.me/webawesome/dist/styles/webawesome.css";
  import "../app.wa.js";
  import "../app.css";
  import { commands } from "$lib/bindings";

  import { setHotkeysContext } from "@tanstack/svelte-hotkeys";
  import {
    initHotkeyBridge,
    useCommandRegistry,
    defaultCommands,
  } from "$lib/commands";
  import CommandPalette from "$lib/commands/components/command-palette.svelte";
  import HelpOverlay from "$lib/commands/components/help-overlay.svelte";
  import CommandReconciler from "$lib/commands/components/command-reconciler.svelte";
  import HeldKeysIndicator from "$lib/commands/components/held-keys-indicator.svelte";

  let { children } = $props();

  setHotkeysContext({
    hotkey: {
      preventDefault: true,
      stopPropagation: true,
      ignoreInputs: true,
    },
    hotkeySequence: {
      timeout: 1500,
    },
  });

  const bridge = initHotkeyBridge();
  const registry = useCommandRegistry();

  // Register the default command set once at startup.
  for (const c of defaultCommands) registry.register(c);
</script>

<CommandReconciler customProfile={bridge.getCustomProfile()} />

<wa-page>
  <nav class="main-header"></nav>
  <main>
    {@render children()}
  </main>
</wa-page>

<CommandPalette {bridge} />
<HelpOverlay />
<HeldKeysIndicator />

<style>
  main {
    padding: 0;
  }
</style>

