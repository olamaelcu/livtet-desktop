<script lang="ts">
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import LibraryToolbar from '../../../lib/library/LibraryToolbar.svelte'
import { sentinel } from '../../../lib/library/sentinel'
import LibraryLayout from '../../../routes/library/LibraryLayout.svelte'

interface Props {
  empty?: boolean
  chips?: number
  selecting?: boolean
  onvisible?: () => void
}

let { empty = false, chips = 0, selecting = false, onvisible = () => {} }: Props = $props()
let scrollElement = $state<HTMLElement>()
</script>

<AppFrame>
  <LibraryLayout bind:scrollElement>
    {#snippet toolbar()}
      <LibraryToolbar
        onaddbook={() => {}}
        selectionMode={selecting}
        ontoggleselect={() => {}}
        coverSize="medium"
        oncoversizechange={() => {}}
      />
    {/snippet}
    {#snippet strip()}
      <div class="chips">
        {#each Array.from({ length: chips }, (_, i) => i) as i (i)}
          <button type="button">A rather long filter chip {i}</button>
        {/each}
      </div>
    {/snippet}
    {#if empty}
      <div class="empty" data-testid="empty">No books? No results.</div>
    {:else}
      <div style="height: 3000px">tall</div>
      <div data-testid="sentinel" use:sentinel={{ root: scrollElement, onvisible }}>end</div>
    {/if}
    {#snippet dock()}
      {#if selecting}
        <button type="button">Delete</button>
        <button type="button">Clear</button>
      {/if}
    {/snippet}
  </LibraryLayout>
</AppFrame>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
  }
</style>
