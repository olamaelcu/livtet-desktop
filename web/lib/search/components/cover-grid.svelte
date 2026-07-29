<script lang="ts">
  import type { SearchHit } from "../types";
  import CoverCard from "./cover-card.svelte";

  interface Props {
    hits: SearchHit[];
  }

  let { hits }: Props = $props();
</script>

<div class="cover-grid">
  <!-- TODO: Include hit.kind (and likely hit.edition_id) in this key when real mixed-kind hits are wired up. -->
  {#each hits as hit (hit.work_id)}
    <CoverCard {hit} />
  {/each}
</div>

<style>
  .cover-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 1rem;
  }
</style>
