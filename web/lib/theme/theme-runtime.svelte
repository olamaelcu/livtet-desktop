<script lang="ts">
  import { activeTheme } from './active-theme.svelte';
  import { onMount } from 'svelte';

  onMount(() => {
    activeTheme.load();
  });

  $: mode = activeTheme.settings.mode;
  $: {
    const html = document.documentElement;
    const isDark =
      mode === 'dark' ||
      (mode === 'auto' &&
        window.matchMedia('(prefers-color-scheme: dark)').matches);
    html.classList.toggle('wa-dark', isDark);
  }
</script>

<svelte:head>
  <style>
    :root {{
      {#each Object.entries(activeTheme.tokens), [k, v]}
        {k}: {v};
      {/each}
    }
  </style>
</svelte:head>
