<script lang="ts">
import AppFrame from '../../../lib/layout/AppFrame.svelte'
import CatalogDetailLayout from '../../../routes/catalog/[catalogId]/CatalogDetailLayout.svelte'
</script>

<AppFrame>
  <CatalogDetailLayout>
    {#snippet toolbar()}
      <wa-button>Back to root</wa-button>
    {/snippet}
    <div style="height: 3000px">publications</div>
  </CatalogDetailLayout>
</AppFrame>
