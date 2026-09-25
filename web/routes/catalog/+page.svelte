<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { loadCatalogs, opdsErrorMessage } from '../../lib/opds'
import { opdsKeys } from '../../lib/query/keys'

const catalogs = createQuery(() => ({
  queryKey: opdsKeys.catalogs(),
  queryFn: loadCatalogs,
}))
</script>

<main class="catalogs">
  <header class="catalogs-header">
    <h2>Catalogs</h2>
    <a class="manage" href="/settings">Manage catalogs</a>
  </header>

  {#if catalogs.isPending}
    <wa-spinner></wa-spinner>
  {:else if catalogs.isError}
    <wa-callout variant="danger">{opdsErrorMessage(catalogs.error)}</wa-callout>
  {:else if (catalogs.data ?? []).length === 0}
    <p class="empty">
      No catalogs subscribed. Add one in <a href="/settings">Settings → Catalogs</a>.
    </p>
  {:else}
    <div class="catalog-grid">
      {#each catalogs.data ?? [] as catalog (catalog.id)}
        <wa-card>
          <a class="catalog-link" href={`/catalog/${catalog.id}`}>
            <strong>{catalog.title}</strong>
            <span class="catalog-url">{catalog.feed_url}</span>
            {#if catalog.auth_kind !== 'none'}
              <wa-badge>{catalog.auth_kind}</wa-badge>
            {/if}
          </a>
        </wa-card>
      {/each}
    </div>
  {/if}
</main>

<style>
  .catalogs {
    padding: var(--wa-space-xl);
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .catalogs-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
  }

  .catalogs-header h2 {
    margin: 0;
  }

  .catalog-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: var(--wa-space-m);
  }

  .catalog-link {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    padding: var(--wa-space-s);
    color: inherit;
    text-decoration: none;
    outline: none;
  }

  .catalog-link:hover,
  .catalog-link:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .catalog-url,
  .empty {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    overflow-wrap: anywhere;
  }
</style>
