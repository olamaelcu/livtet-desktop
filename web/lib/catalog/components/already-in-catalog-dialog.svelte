<script lang="ts">
import { catalogMatchState, closeCatalogMatches } from '../catalog-match-state.svelte'
import { openPeek } from '../peek-state.svelte'

function openEdition(editionId: string | null | undefined): void {
  if (editionId) {
    closeCatalogMatches()
    openPeek(editionId)
  }
}
</script>

<wa-dialog
  open={catalogMatchState.open}
  label="Already in your catalog"
  light-dismiss
  onwa-after-hide={closeCatalogMatches}
>
  <p>
    {catalogMatchState.matches.length === 1
      ? 'This book is already in your catalog:'
      : `These ${catalogMatchState.matches.length} books are already in your catalog:`}
  </p>

  <ul class="match-list">
    {#each catalogMatchState.matches as hit}
      <li class="match-item">
        <span class="match-title">{hit.title}</span>
        {#if hit.authors.length > 0}
          <span class="match-authors">by {hit.authors.join(', ')}</span>
        {/if}
        <wa-button
          size="s"
          appearance="outlined"
          onclick={() => openEdition(hit.in_catalog_edition_id)}
        >
          Open
        </wa-button>
      </li>
    {/each}
  </ul>

  <wa-button
    slot="footer"
    appearance="neutral"
    onclick={closeCatalogMatches}
  >
    Dismiss
  </wa-button>
</wa-dialog>

<style>
  wa-dialog::part(panel) {
    width: min(40rem, 90vw);
  }

  .match-list {
    list-style: none;
    padding: 0;
    margin: var(--wa-space-m) 0 0 0;
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .match-item {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
  }

  .match-title {
    font-weight: 600;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .match-authors {
    font-size: 0.875em;
    color: var(--wa-color-neutral-600);
    white-space: nowrap;
  }
</style>
