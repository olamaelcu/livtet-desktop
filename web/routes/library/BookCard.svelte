<script lang="ts">
interface Props {
  title: string
  cover_url?: string
  selectable?: boolean
  selected?: boolean
  /** File availability: `true` = on disk, `false` = virtual/remote. */
  in_filesystem?: boolean
  onclick?: () => void
  ondblclick?: () => void
}

let {
  title,
  cover_url = undefined,
  selectable = false,
  selected = false,
  in_filesystem = undefined,
  onclick,
  ondblclick,
}: Props = $props()

let failedUrl = $state<string | undefined>(undefined)
const showCover = $derived(cover_url !== undefined && failedUrl !== cover_url)
</script>

<div class="card-wrap">
  <button
    class="card {selectable && selected ? 'is-selected' : ''}"
    type="button"
    aria-pressed={selectable ? selected : undefined}
    {onclick}
    {ondblclick}
  >
    <figure class="cover {showCover ? '' : 'placeholder'}">
      {#if showCover}
        <img src={cover_url} alt={title} onerror={() => (failedUrl = cover_url)} />
      {:else}
        <figcaption>{title}</figcaption>
      {/if}
    </figure>
  </button>
  {#if selectable && selected}
    <span class="check-badge" aria-hidden="true"><wa-icon name="check"></wa-icon></span>
  {/if}
  {#if in_filesystem !== undefined}
    <span
      class="file-badge {in_filesystem ? 'is-local' : 'is-remote'}"
      role="img"
      aria-label={in_filesystem ? 'In filesystem' : 'Not in filesystem'}
      title={in_filesystem ? 'In filesystem' : 'Not in filesystem'}
    >
      <wa-icon name={in_filesystem ? 'hard-drive' : 'cloud'}></wa-icon>
    </span>
  {/if}
</div>

<style>
  .card-wrap {
    position: relative;
    display: block;
    min-width: 0;
    padding: var(--wa-space-2xs);
  }

  .card {
    all: unset;
    display: block;
    width: 100%;
    cursor: pointer;
    transition: box-shadow 0.15s ease;
  }

  .card:hover,
  .card:focus-visible {
    box-shadow: var(--wa-focus-ring);
  }

  .cover {
    box-sizing: border-box;
    width: 100%;
    aspect-ratio: 3 / 4;
    height: auto;
    overflow: hidden;
    border: 0.0625rem solid var(--wa-color-border-default);
    background: var(--wa-color-surface-alt);
    margin: 0;
    padding: 0;
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .placeholder {
    background: linear-gradient(135deg, #2d323f 0%, #3a3f4d 100%);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    font-size: 0.65rem;
    color: var(--wa-color-text-secondary);
    text-align: center;
  }

  .placeholder figcaption {
    padding: var(--wa-space-s);
    overflow: hidden;
  }

  .card.is-selected .cover {
    outline: 2px solid var(--wa-color-brand-fill-loud);
    outline-offset: 2px;
  }

  .check-badge {
    position: absolute;
    top: var(--wa-space-2xs);
    left: var(--wa-space-2xs);
    z-index: 1;
    display: grid;
    place-items: center;
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 50%;
    background: var(--wa-color-brand-fill-loud);
    color: var(--wa-color-brand-on-loud);
    font-size: var(--wa-font-size-2xs, 0.75rem);
  }

  .file-badge {
    position: absolute;
    top: var(--wa-space-2xs);
    right: var(--wa-space-2xs);
    z-index: 1;
    display: grid;
    place-items: center;
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 50%;
    border: 1px solid var(--wa-color-border-default);
    background: var(--wa-color-surface-default);
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-2xs, 0.75rem);
  }

  .file-badge.is-local {
    color: var(--wa-color-brand-fill-loud);
  }
</style>
