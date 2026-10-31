<script lang="ts">
import {
  type ClickDisambiguation,
  createClickDisambiguation,
} from '../../lib/library/clickDisambiguation'

interface Props {
  title: string
  cover_url?: string
  selectable?: boolean
  selected?: boolean
  /** File availability: `true` = on disk, `false` = virtual/remote. */
  in_filesystem?: boolean
  onclick?: () => void
  ondblclick?: () => void
  /** Extra declarations, e.g. `--i` so the shelf can stagger its entrance. */
  style?: string
}

let {
  title,
  cover_url = undefined,
  selectable = false,
  selected = false,
  in_filesystem = undefined,
  onclick,
  ondblclick,
  style = undefined,
}: Props = $props()

let failedUrl = $state<string | undefined>(undefined)
const showCover = $derived(cover_url !== undefined && failedUrl !== cover_url)

let press = $state<ClickDisambiguation | undefined>(undefined)

$effect(() => {
  const next = createClickDisambiguation({
    onSingle: () => onclick?.(),
    onDouble: ondblclick,
  })
  press = next
  return () => next.dispose()
})
</script>

<div class="card-wrap" {style}>
  <button
    class="card {selectable && selected ? 'is-selected' : ''}"
    type="button"
    aria-pressed={selectable ? selected : undefined}
    onclick={(event) => press?.handleClick(event)}
    ondblclick={() => press?.handleDoubleClick()}
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
      aria-label={in_filesystem ? 'In filesystem' : 'Virtual book'}
      title={in_filesystem ? 'In filesystem' : 'Virtual book'}
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

  /* New results settle in rather than appearing: the shelf passes `--i` so the
     stagger follows reading order. Capped so a deep row is not left waiting. */
  @media (prefers-reduced-motion: no-preference) {
    .card-wrap {
      animation: card-settle 260ms cubic-bezier(0.2, 0.7, 0.2, 1) both;
      animation-delay: calc(min(var(--i, 0), 12) * 18ms);
    }
  }

  @keyframes card-settle {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.985);
    }
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
    /* The only literal colours the app had; follow the theme tokens so the
       placeholder cover darkens with the rest of the surface palette. */
    background: linear-gradient(
      135deg,
      var(--wa-color-surface-lowered) 0%,
      var(--wa-color-surface-raised) 100%
    );
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
    top: var(--wa-space-xs);
    right: var(--wa-space-xs);
    z-index: 1;
    display: grid;
    place-items: center;
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 50%;
    border: 1px solid var(--wa-color-border-default);
    background: var(--wa-color-overlay-inline);
    color: var(--wa-color-overlay-modal);
    font-size: var(--wa-font-size-2xs, 0.75rem);
  }

  .file-badge.is-local {
    color: var(--wa-color-brand-fill-loud);
  }
</style>
