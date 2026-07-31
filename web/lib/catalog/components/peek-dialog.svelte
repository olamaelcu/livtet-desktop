<script lang="ts">
  import { peekState, closePeek } from "../peek-state.svelte";
  import EditionDetail from "./edition-detail.svelte";
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
</wa-dialog>

<style>
  wa-dialog::part(panel) {
    width: min(60rem, 95vw);
    max-height: 85vh;
  }
</style>