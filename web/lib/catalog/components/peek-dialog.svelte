<script lang="ts">
import { goto } from '$app/navigation'
import { closePeek, peekState } from '../peek-state.svelte'
import EditionDetail from './edition-detail.svelte'
</script>

<wa-dialog
  open={peekState.open}
  label="Edition details"
  light-dismiss
  onwa-after-hide={closePeek}
>
  {#if peekState.editionId}
    <EditionDetail editionId={peekState.editionId} />
  {/if}
  <footer slot="footer">
    {#if peekState.editionId}
      <wa-button
        appearance="outlined"
        onclick={() => {
          const id = peekState.editionId
          closePeek()
          if (id) goto('/catalog/' + id)
        }}
      >
        Open full page
      </wa-button>
    {/if}
  </footer>
</wa-dialog>

<style>
  wa-dialog::part(panel) {
    width: min(60rem, 95vw);
    max-height: 85vh;
  }
</style>