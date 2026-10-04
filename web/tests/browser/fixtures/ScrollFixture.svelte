<script lang="ts">
import Pane from '../../../lib/layout/Pane.svelte'
import ScrollRegion from '../../../lib/layout/ScrollRegion.svelte'

let { onscroll }: { onscroll?: () => void } = $props()
</script>

<div id="host" style="height: 200px; display: grid" {onscroll}>
  <Pane>
    <ScrollRegion>
      <div style="height: 1000px">tall</div>
    </ScrollRegion>
  </Pane>
</div>
