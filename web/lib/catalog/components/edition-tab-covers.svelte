<script lang="ts">
  import { toast } from "svelte-sonner";
  import { type CachedCover, commands } from "$lib/bindings";
  import { triggerCoverRefresh } from "$lib/search/cover-refresh.svelte";

  interface Props {
    editionId: string;
  }

  let { editionId }: Props = $props();

  let covers = $state<CachedCover[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let fetching = $state(false);

  function bytesToUrl(bytes: number[]): string {
    const mime =
      bytes[0] === 0xff && bytes[1] === 0xd8
        ? "image/jpeg"
        : bytes[0] === 0x89 && bytes[1] === 0x50
          ? "image/png"
          : bytes[0] === 0x52 && bytes[1] === 0x49
            ? "image/webp"
            : "image/jpeg";
    const blob = new Blob([new Uint8Array(bytes)], { type: mime });
    return URL.createObjectURL(blob);
  }

  const coverUrls = $derived.by(() => {
    const map = new Map<string, string>();
    for (const c of covers) {
      if (c.bytes.length > 0) map.set(c.key, bytesToUrl(c.bytes));
    }
    return map;
  });

  function loadCovers(): void {
    loading = true;
    error = null;
    commands
      .listCovers(editionId)
      .then((res) => {
        if (res.status === "ok") {
          covers = res.data;
          error = null;
        } else {
          error = res.error;
          covers = [];
        }
        loading = false;
      })
      .catch((e: unknown) => {
        error = String(e);
        covers = [];
        loading = false;
      });
  }

  async function fetchCover(e: Event): Promise<void> {
    e.stopPropagation();
    if (fetching) return;
    fetching = true;
    try {
      const res = await commands.fetchCover(editionId);
      if (res.status === "ok") {
        toast.success(`Cover found via ${res.data.provider}`);
        triggerCoverRefresh(editionId);
        loadCovers();
      } else {
        toast.error(res.error);
      }
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Fetch failed");
    } finally {
      fetching = false;
    }
  }

  $effect(() => {
    loadCovers();
  });
</script>

{#if loading}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="hourglass"></wa-icon>
    Loading covers…
  </wa-callout>
{:else if error}
  <wa-callout variant="danger">
    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
    Failed to load: {error}
  </wa-callout>
{:else if covers.length === 0}
  <wa-callout variant="neutral">
    <wa-icon slot="icon" name="circle-info"></wa-icon>
    No covers found for this edition.
  </wa-callout>
{:else}
  <div class="covers-grid">
    {#each covers as cover (cover.key)}
      <article class="cover-item">
        <div
          class="cover-preview"
          style:background={cover.dominant_color ?? undefined}
        >
          {#if coverUrls.get(cover.key)}
            {@const src = coverUrls.get(cover.key)}
            <img {src} alt="" class="cover-img" />
          {:else if cover.blurhash}
            <span class="cover-blurhash">{cover.blurhash}</span>
          {:else}
            <wa-icon name="image" class="cover-placeholder"></wa-icon>
          {/if}
        </div>
        <dl class="cover-meta">
          <div class="meta-row">
            <dt>Provider</dt>
            <dd>{cover.provider}</dd>
          </div>
          <div class="meta-row">
            <dt>Size</dt>
            <dd><wa-badge size="s">{cover.size}</wa-badge></dd>
          </div>
          <div class="meta-row">
            <dt>Format</dt>
            <dd>{cover.ext.toUpperCase()}</dd>
          </div>
        </dl>
      </article>
    {/each}
  </div>
{/if}

<div class="toolbar">
  <wa-button on:click={fetchCover} disabled={fetching}>
    {fetching ? "Fetching…" : "Fetch Cover"}
  </wa-button>
</div>

<style>
  .covers-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
    gap: 1rem;
  }

  .cover-item {
    display: flex;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: var(--wa-border-radius-m, 6px);
    overflow: hidden;
  }

  .cover-preview {
    width: 6rem;
    aspect-ratio: 2 / 3;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    background: var(--wa-color-surface-quiet, #f0f0f0);
  }

  .cover-placeholder {
    font-size: 2rem;
    opacity: 0.4;
  }

  .cover-blurhash {
    font-family: monospace;
    font-size: 0.625rem;
    color: rgba(255, 255, 255, 0.85);
    padding: 0.25rem;
    word-break: break-all;
    text-align: center;
  }

  .cover-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cover-meta {
    flex: 1;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.375rem;
    font-size: 0.8125rem;
  }

  .meta-row {
    display: flex;
    gap: 0.5rem;
  }

  .meta-row dt {
    color: var(--wa-color-text-quiet, currentColor);
    min-width: 4rem;
  }

  .meta-row dd {
    margin: 0;
  }

  .toolbar {
    margin-top: 1rem;
  }
</style>
