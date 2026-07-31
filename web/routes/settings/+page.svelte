<script lang="ts">
import HardcoverKeyForm from '$lib/settings/hardcover-key-form.svelte'
</script>

<svelte:head>
  <title>Settings · livtet</title>
</svelte:head>

<main class="settings">
  <header class="page-header">
    <h1>Settings</h1>
    <p class="hint">Keys live in your OS keychain. We never store them in plain text.</p>
  </header>

  <HardcoverKeyForm />
</main>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    padding: 1.5rem;
    max-width: 48rem;
  }

  .page-header h1 {
    margin: 0 0 0.25rem 0;
    font-size: 1.5rem;
  }

  .hint {
    margin: 0;
    font-size: 0.875rem;
    color: var(--wa-color-text-quiet, currentColor);
  }
</style>