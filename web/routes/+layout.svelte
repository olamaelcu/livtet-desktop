<script>
  import "@awesome.me/webawesome/dist/styles/webawesome.css";
  import "../app.wa.js";
  import "../app.css";
  import { commands } from "$lib/bindings";

  let { children } = $props();

  // Mirror document.title into the OS window chrome. Svelte 5
  // routes update the title via <svelte:head><title>...</title>,
  // which writes to document.title but doesn't itself trigger a
  // reactive read here. A MutationObserver on the <title> element
  // picks up every change (route navigation, programmatic writes,
  // browser-default tab-title assignments) and pushes each one
  // through the Tauri command.
  $effect(() => {
    if (typeof document === "undefined") return;

    const sync = () => commands.syncWindowTitle(document.title);
    sync();

    const title = document.head.querySelector("title");
    if (!title) return;

    const observer = new MutationObserver(sync);
    observer.observe(title, {
      childList: true,
      characterData: true,
      subtree: true,
    });

    return () => observer.disconnect();
  });
</script>

<wa-page>
  <nav class="main-header"></nav>
  <main>
    {@render children()}
  </main>
</wa-page>

<style>
  main {
    padding: var(--wa-space-s);
  }
</style>
