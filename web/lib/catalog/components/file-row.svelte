<script lang="ts">
  import type { DigitalInventoryRow } from "$lib/bindings";

  interface Props {
    file: DigitalInventoryRow;
  }

  let { file }: Props = $props();

  const basename = $derived.by(() => {
    const path = file.file_path;
    if (!path) return "(no path)";
    const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    return idx >= 0 ? path.slice(idx + 1) : path;
  });

  const parentDir = $derived.by(() => {
    const path = file.file_path;
    if (!path) return "";
    const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    return idx >= 0 ? path.slice(0, idx) : "";
  });

  const sizeLabel = $derived.by(() => {
    if (file.file_size_bytes == null) return "";
    const fmt = new Intl.NumberFormat(undefined, {
      style: "unit",
      unit: "byte",
      unitDisplay: "narrow",
      maximumFractionDigits: 1,
    });
    return fmt.format(file.file_size_bytes);
  });

  const truncatedHash = $derived.by(() => {
    const h = file.file_hash;
    if (!h) return "";
    return h.length > 12 ? `${h.slice(0, 6)}…${h.slice(-4)}` : h;
  });
</script>

<article class="file-row">
  <div class="preview" aria-hidden="true">
    {#if file.dominant_color}
      <div class="swatch" style:--dominant={file.dominant_color}></div>
    {:else}
      <div class="swatch swatch--placeholder"></div>
    {/if}
  </div>

  <div class="meta">
    <div class="filename" title={file.file_path ?? ""}>{basename}</div>
    {#if parentDir}
      <div class="parent" title={parentDir}>{parentDir}</div>
    {/if}
    <div class="line">
      {#if sizeLabel}<span class="size">{sizeLabel}</span>{/if}
      {#if truncatedHash}<code class="hash" title={file.file_hash}
          >{truncatedHash}</code
        >{/if}
    </div>
    {#if file.notes}
      <wa-callout variant="neutral" class="notes">
        {file.notes}
      </wa-callout>
    {/if}
  </div>
</article>

<style>
  .file-row {
    display: grid;
    grid-template-columns: 4rem 1fr;
    gap: var(--wa-space-m, 0.75rem);
    padding: var(--wa-space-s, 0.5rem);
    align-items: start;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 6px;
    background: var(--wa-color-surface-default, transparent);
  }

  .preview {
    aspect-ratio: 2 / 3;
    background: var(--wa-color-surface-lowered, rgba(0, 0, 0, 0.05));
    border-radius: 4px;
    overflow: hidden;
    display: grid;
    place-items: center;
  }

  .swatch {
    width: 100%;
    height: 100%;
    background: var(--dominant, var(--wa-color-surface-lowered, #888));
  }

  .meta {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .filename {
    font-weight: 600;
    font-size: 0.9375rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .parent {
    font-size: 0.75rem;
    color: var(--wa-color-text-quiet, currentColor);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .line {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-size: 0.8125rem;
    color: var(--wa-color-text-quiet, currentColor);
  }

  .size {
    font-variant-numeric: tabular-nums;
  }

  .hash {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.75rem;
  }

  wa-callout.notes {
    margin-top: 0.5rem;
  }
</style>