<script lang="ts">
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import ListLayout from '../../../lib/layout/ListLayout.svelte'
</script>

<AppFrame>
  <ListLayout title="Things">
    {#snippet action()}
      <wa-button>Act</wa-button>
    {/snippet}
    <div style="height: 3000px">tall</div>
  </ListLayout>
</AppFrame>
