<script>
  import "@awesome.me/webawesome/dist/styles/webawesome.css";
  import "../app.wa.js";
  import "../app.css";
  import { commands } from "$lib/bindings";

  let { children } = $props();

  // Mirror document.title into the OS window chrome. Each
  // <svelte:head><title> change in a route fires this effect and
  // pushes the new title to the Tauri-side WebviewWindow.
  $effect(() => {
    if (typeof document !== "undefined") {
      commands.syncWindowTitle(document.title);
    }
  });
</script>

<wa-page>
  <nav class="main-header">
  </nav>
  <main>
    {@render children()}
  </main>
</wa-page>

<style>
  main {
    padding: var(--wa-space-s);
  }
</style>
