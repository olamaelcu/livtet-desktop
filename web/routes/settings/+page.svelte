<script lang="ts">
import HotkeySettings from '../../lib/components/HotkeySettings.svelte'
import SyncPanel from '../../lib/components/SyncPanel.svelte'
</script>

<main class="settings">
  <section aria-labelledby="sync-settings-heading">
    <h2 id="sync-settings-heading">Sync</h2>
    <SyncPanel />
  </section>

  <HotkeySettings />
</main>

<style>
  .settings {
    padding: var(--wa-space-xl);
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xl);
  }

  .settings h2 {
    margin: 0 0 var(--wa-space-l);
  }
</style>
