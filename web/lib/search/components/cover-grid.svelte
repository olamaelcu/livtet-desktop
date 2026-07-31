<script lang="ts">
import type { SearchHit } from '../types'
import CoverCard from './cover-card.svelte'

interface Props {
  hits: readonly SearchHit[]
  onremove?: (digitalInventoryId: string) => void
  badge?: import('svelte').Snippet<[hit: SearchHit]>
  actions?: import('svelte').Snippet<[hit: SearchHit]>
}

let { hits, onremove, badge, actions }: Props = $props()
</script>

<div class="cover-grid">
  {#each hits as hit (hit.work_id)}
    <CoverCard {hit} {onremove} badge={badge} actions={actions} />
  {/each}
</div>

<style>
  .cover-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 1rem;
  }
</style>
