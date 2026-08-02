<script lang="ts">
  import { searchFonts, type FontMeta } from './fontsource-api';
  import { commands } from '$lib/bindings';
  import type { FontHandle } from '$lib/bindings';
  import { activeTheme } from '$lib/theme/active-theme.svelte';
  import type { FontSlot } from '$lib/theme/types';
  import { toast } from 'svelte-sonner';

  const SLOT_LABEL: Record<FontSlot, string> = {
    body: 'Body',
    heading: 'Heading',
    code: 'Code',
    longform: 'Longform',
  };

  interface Props {
    slot: FontSlot;
    label?: string;
  }

  let { slot, label }: Props = $props();
  const title = $derived(label ?? SLOT_LABEL[slot]);

  const current = $derived(activeTheme.settings.overrides.fontSlots?.[slot] ?? '');

  let downloaded = $state<FontHandle[]>([]);
  let query = $state('');
  let results = $state<FontMeta[]>([]);
  let loading = $state(false);

  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  async function refreshDownloaded() {
    const r = await commands.listDownloadedFonts();
    if (r.status === 'ok') downloaded = r.data;
  }

  function oninput(e: Event) {
    // TS-only cast: the runtime target is the <wa-input> custom element.
    const target = e.target as HTMLInputElement | null;
    if (target) query = target.value;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(doSearch, 300);
  }

  async function doSearch() {
    if (!query.trim()) {
      results = [];
      return;
    }
    loading = true;
    try {
      results = await searchFonts(query);
    } catch (e) {
      toast.error(String(e));
    } finally {
      loading = false;
    }
  }

  function assign(family: string) {
    activeTheme.update({
      overrides: {
        ...activeTheme.settings.overrides,
        fontSlots: {
          ...(activeTheme.settings.overrides.fontSlots ?? {}),
          [slot]: family,
        },
      },
    });
  }

  async function pick(meta: FontMeta) {
    if (!downloaded.some((d) => d.familyId === meta.id)) {
      const r = await commands.downloadFont(meta.id, ['latin'], ['400']);
      if (r.status === 'ok') {
        await refreshDownloaded();
      } else {
        toast.error(r.error);
        return;
      }
    }
    assign(meta.family);
  }

  function pickDownloaded(family: string) {
    assign(family);
  }

  refreshDownloaded();
</script>

<div class="font-slot-picker">
  <div class="slot-header">
    <span class="slot-title">{title}</span>
    {#if current}
      <wa-badge appearance="filled" variant="neutral">{current}</wa-badge>
    {:else}
      <span class="slot-placeholder">Uses preset default</span>
    {/if}
  </div>

  <wa-input
    placeholder="Search Fontsource fonts…"
    value={query || ''}
    oninput={oninput}
    type="search"
  >
    <wa-icon slot="start" name="magnifying-glass"></wa-icon>
    {#if loading}
      <wa-spinner slot="end" size="s"></wa-spinner>
    {/if}
  </wa-input>

  {#if query.trim()}
    <div class="font-results">
      {#each results as r (r.id)}
        <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
        <!--   wa-button renders a native <button> (button semantics + Enter/Space);
             Svelte's analyzer does not recognize WA custom elements. -->
        <wa-button appearance="plain" onclick={() => pick(r)}>
          <div class="font-result-item">
            <span>{r.family}</span>
            <wa-badge variant={r.variable ? 'brand' : 'neutral'} appearance="filled">
              {r.variable ? 'Variable' : r.category}
            </wa-badge>
          </div>
        </wa-button>
      {/each}
    </div>
  {:else if downloaded.length > 0}
    <div class="font-results">
      <div class="results-label">Downloaded</div>
      {#each downloaded as d (d.familyId)}
        <!-- svelte-ignore a11y_no_static_element_interactions,a11y_click_events_have_key_events -->
        <!--   wa-button renders a native <button> (button semantics + Enter/Space);
             Svelte's analyzer does not recognize WA custom elements. -->
        <wa-button appearance="plain" onclick={() => pickDownloaded(d.family)}>
          <div class="font-result-item">
            <span>{d.family}</span>
            <wa-badge variant="neutral" appearance="filled">v{d.version}</wa-badge>
          </div>
        </wa-button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .font-slot-picker {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs, 0.5rem);
  }

  .slot-header {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs, 0.5rem);
    font-weight: 600;
  }

  .slot-placeholder {
    color: var(--wa-color-neutral-10, #6b7280);
    font-weight: 400;
    font-size: 0.875rem;
  }

  wa-input {
    width: 100%;
  }

  .font-results {
    max-height: 160px;
    overflow-y: auto;
  }

  .results-label {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--wa-color-neutral-10, #6b7280);
    padding: var(--wa-space-3xs, 0.25rem) var(--wa-space-2xs, 0.5rem);
  }

  .font-result-item {
    display: flex;
    justify-content: space-between;
    width: 100%;
    align-items: center;
  }
</style>
