<script lang="ts">
import NavRail from '../../../lib/layout/NavRail.svelte'
import { NAV_ITEMS } from '../../../lib/layout/navItems'

let { width }: { width: string } = $props()
</script>

<div data-testid="container" style="container-type: inline-size; width: {width}">
  <NavRail items={NAV_ITEMS} currentPath="/library" expanded />
</div>
