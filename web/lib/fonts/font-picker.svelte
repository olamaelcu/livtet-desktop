<script lang="ts">
  import { searchFonts } from './fontsource-api';
  import type { FontMeta } from './fontsource-api';
  import { toast } from 'svelte-sonner';

  interface Props {
    onpick: (meta: FontMeta) => void;
    placeholder?: string;
  }
  let { onpick, placeholder = "Search fonts…" }: Props = $props();
  let query = $state('');
  let results = $state<FontMeta[]>([]);
  let loading = $state(false);

  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  function oninput(e: Event) {
    // TS-only cast: the runtime target is the <wa-input> custom element.
    const target = e.target as HTMLInputElement | null;
    if (target) query = target.value;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(doSearch, 300);
  }

  async function doSearch() {
    if (!query.trim()) { results = []; return; }
    loading = true;
    try {
      results = await searchFonts(query);
    } catch (e) {
      toast.error(String(e));
    } finally {
      loading = false;
    }
  }
</script>

<wa-input {placeholder} value={query || ""} oninput={oninput} type="search">
  <wa-icon slot="start" name="magnifying-glass"></wa-icon>
  {#if loading}
    <wa-spinner slot="end" size="s"></wa-spinner>
  {/if}
</wa-input>

{#if results.length > 0}
  <div class="font-results">
    {#each results as r (r.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
      <!--   wa-button is interactive (button semantics); Svelte's analyzer
           doesn't recognize WA custom elements. -->
      <wa-button appearance="plain" onclick={() => onpick(r)}>
        <div class="font-result-item">
          <span>{r.family}</span>
          <wa-badge variant={r.variable ? "brand" : "neutral"} appearance="filled">
            {r.variable ? "Variable" : r.category}
          </wa-badge>
        </div>
      </wa-button>
    {/each}
  </div>
{/if}

<style>
  .font-results {
    max-height: 200px;
    overflow-y: auto;
  }

  .font-result-item {
    display: flex;
    justify-content: space-between;
    width: 100%;
    align-items: center;
  }
</style>
