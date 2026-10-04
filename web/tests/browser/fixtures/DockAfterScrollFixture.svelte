<script lang="ts">
import Dock from '../../../lib/layout/Dock.svelte'
import Pane from '../../../lib/layout/Pane.svelte'
import ScrollRegion from '../../../lib/layout/ScrollRegion.svelte'
</script>

<div id="host" style="height: 200px; display: grid">
  <Pane>
    <ScrollRegion>
      <div style="height: 1000px">tall</div>
    </ScrollRegion>
    <Dock>dock</Dock>
  </Pane>
</div>
