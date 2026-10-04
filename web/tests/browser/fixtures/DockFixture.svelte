<script lang="ts">
import Dock from '../../../lib/layout/Dock.svelte'
import Pane from '../../../lib/layout/Pane.svelte'
</script>

<div style="padding: 80px">
  <div style="height: 200px; display: grid">
    <Pane>
      <Dock><span data-testid="dock-content">dock</span></Dock>
    </Pane>
  </div>
</div>
