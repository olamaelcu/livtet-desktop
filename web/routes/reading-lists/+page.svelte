<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import ListLayout from '../../lib/layout/ListLayout.svelte'
import { readingListKeys } from '../../lib/query/keys'
import { listReadingLists, readingListErrorMessage } from '../../lib/reading-lists'

const lists = createQuery(() => ({
  queryKey: readingListKeys.lists(),
  queryFn: listReadingLists,
}))
</script>

<ListLayout title="Reading lists">
  {#if lists.isPending}
    <wa-spinner></wa-spinner>
  {:else if lists.isError}
    <wa-callout variant="danger">
      {readingListErrorMessage(lists.error)}
    </wa-callout>
  {:else if (lists.data ?? []).length === 0}
    <p class="empty">No reading lists yet.</p>
  {:else}
    <ul class="list-grid">
      {#each lists.data ?? [] as list (list.id)}
        <li>
          <wa-card>
            <a class="list-link" href={`/reading-lists/${list.id}`}>
              <strong>{list.name}</strong>
              {#if list.description}
                <span class="description">{list.description}</span>
              {/if}
              <span class="count">
                {list.edition_ids.length}
                {list.edition_ids.length === 1 ? 'book' : 'books'}
              </span>
            </a>
          </wa-card>
        </li>
      {/each}
    </ul>
  {/if}
</ListLayout>

<style>
  .list-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: var(--wa-space-m);
    padding: 0;
    margin: 0;
    list-style: none;
  }

  .list-link {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    padding: var(--wa-space-s);
    color: inherit;
    text-decoration: none;
    outline: none;
  }

  .list-link:hover,
  .list-link:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .description {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-s);
    overflow-wrap: anywhere;
  }

  .count,
  .empty {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }
</style>
