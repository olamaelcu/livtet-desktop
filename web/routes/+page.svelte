<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let name = $state("");
  let greetMsg = $state("");
  let loading = $state(false);

  async function greet(event: SubmitEvent) {
    event.preventDefault();
    if (!name.trim()) return;
    try {
      loading = true;
      greetMsg = await invoke("greet", { name });
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>livtet</title>
</svelte:head>

<main class="page">
  <wa-card class="card">
    <div slot="header" class="card-header">
      <wa-icon name="house" variant="solid" class="mark"></wa-icon>
      <div class="title-group">
        <h1>livtet</h1>
        <p class="subtitle">A coherent interface for daily work.</p>
      </div>
    </div>

    <form class="form" onsubmit={greet}>
      <wa-input
        label="Your name"
        placeholder="Ada Lovelace"
        value={name}
        oninput={(e) => (name = (e.target as HTMLInputElement).value)}
        required
      ></wa-input>
      <wa-button type="submit" variant="brand" loading={loading} disabled={!name.trim()}>
        Greet
        <wa-icon slot="end" name="arrow-right"></wa-icon>
      </wa-button>
    </form>

    {#if greetMsg}
      <wa-callout variant="success" class="greeting">
        <wa-icon slot="icon" name="check-circle" variant="solid"></wa-icon>
        {greetMsg}
      </wa-callout>
    {/if}
  </wa-card>
</main>

<style>
  .page {
    min-height: 100vh;
    display: grid;
    place-items: center;
    padding: 2rem 1.25rem;
    box-sizing: border-box;
  }

  .card {
    width: 100%;
    max-width: 28rem;
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 0.875rem;
  }

  .card-header .mark {
    font-size: 1.75rem;
    color: var(--wa-color-brand-text-quiet);
  }

  .title-group h1 {
    margin: 0;
    font-size: 1.375rem;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .subtitle {
    margin: 0.125rem 0 0;
    font-size: 0.9375rem;
    color: var(--wa-color-text-quiet);
  }

  .form {
    display: flex;
    gap: 0.75rem;
    align-items: end;
    margin-top: 1.25rem;
  }

  .form wa-input {
    flex: 1;
  }

  .greeting {
    margin-top: 1.25rem;
  }
</style>
