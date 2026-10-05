<svelte:head>
  <title>livtet</title>
</svelte:head>

<script lang="ts">
import { onMount } from 'svelte'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'
import RouteShell from '../lib/layout/RouteShell.svelte'

onMount(() => {
  setTimeout(() => goto(resolve('/library')), 1500)
})
</script>

<RouteShell>
  <main>
  <wa-callout>
    <wa-spinner slot="icon"></wa-spinner>
      Preparing your experience with Livtet&hellip;
  </wa-callout>
  </main>
</RouteShell>
