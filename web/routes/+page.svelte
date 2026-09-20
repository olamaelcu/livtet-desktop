<svelte:head>
  <title>livtet</title>
</svelte:head>

<script lang="ts">
import { onMount } from 'svelte'
import { goto } from '$app/navigation'
import { resolve } from '$app/paths'

onMount(() => {
  setTimeout(() => goto(resolve('/library')), 1500)
})
</script>

<main>
  <wa-callout>
    <wa-spinner slot="icon"></wa-spinner>
      Preparing your experience with Livtet&hellip;
  </wa-callout>
</main>
