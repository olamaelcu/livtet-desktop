<script lang="ts"></script>

<svelte:head>
  <title>livtet</title>
</svelte:head>

<main class="page">
  <wa-card class="card">
    <wa-button href="/search">Search Local Catalog</wa-button>
    <wa-button href="/search/remote">Search Remote Catalog</wa-button>
  </wa-card>
</main>

<style>
  .page {
    min-height: 100vh;
    width: 100%;
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
</style>
