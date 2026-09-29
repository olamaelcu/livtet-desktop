<script lang="ts">
import AddPluginPanel from '../../lib/components/AddPluginPanel.svelte'
import PluginList from '../../lib/components/PluginList.svelte'
</script>

<main class="plugins-page">
  <header>
    <h1>Plugins</h1>
    <p class="muted">
      Plugins extend Livtet with importers and providers. They run out-of-process
      and sandboxed — review each plugin's signer and the capabilities it was
      granted.
    </p>
  </header>

  <section>
    <h2>Add a plugin</h2>
    <AddPluginPanel />
  </section>

  <section>
    <h2>Installed</h2>
    <PluginList />
  </section>
</main>

<style>
  .plugins-page {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xl);
    padding: var(--wa-space-xl);
    max-width: 56rem;
  }

  header h1 {
    margin: 0 0 var(--wa-space-2xs);
  }

  section h2 {
    margin: 0 0 var(--wa-space-m);
    font-size: var(--wa-font-size-l);
  }

  .muted {
    color: var(--wa-color-text-quiet);
    margin: 0;
  }
</style>
