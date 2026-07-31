<script lang="ts">
import { attachAsButton } from '$lib/a11y/attachments'
import { commands } from '$lib/bindings'
import { editionForHit } from '$lib/catalog/edition-for-hit'
import { openPeek } from '$lib/catalog/peek-state.svelte'
import { coverLetter, dominantColorFor } from '../cover-art'
import { consumeCoverRefresh, refreshState } from '../cover-refresh.svelte'
import type { SearchHit } from '../types'

interface Props {
  hit: SearchHit
  onremove?: (digitalInventoryId: string) => void
  badge?: import('svelte').Snippet<[hit: SearchHit]>
  actions?: import('svelte').Snippet<[hit: SearchHit]>
}

let { hit, onremove, badge, actions }: Props = $props()

const bg = $derived(hit.dominant_color ?? dominantColorFor(hit.title))
const letter = $derived(coverLetter(hit.title))
const authorsLine = $derived(hit.authors.length === 0 ? '' : hit.authors.join(', '))

let activating = $state(false)
let coverSrc: string | null = $state(null)

function bytesToUrl(bytes: number[]): string {
  const mime =
    bytes[0] === 0xff && bytes[1] === 0xd8
      ? 'image/jpeg'
      : bytes[0] === 0x89 && bytes[1] === 0x50
        ? 'image/png'
        : bytes[0] === 0x52 && bytes[1] === 0x49
          ? 'image/webp'
          : 'image/jpeg'
  const blob = new Blob([new Uint8Array(bytes)], { type: mime })
  return URL.createObjectURL(blob)
}

$effect(() => {
  void refreshState.version
  if (!hit.edition_id) return
  if (coverSrc && !consumeCoverRefresh(hit.edition_id)) return
  let cancelled = false
  commands
    .listCovers(hit.edition_id)
    .then((res) => {
      if (cancelled) return
      if (res.status === 'ok' && res.data.length > 0) {
        const first = res.data[0]
        if (first.bytes.length > 0) {
          coverSrc = bytesToUrl(first.bytes)
        }
      }
    })
    .catch(() => {})
  return () => {
    cancelled = true
  }
})

async function onActivate(): Promise<void> {
  if (activating) return
  activating = true
  try {
    const filesRes =
      hit.work_id && hit.edition_id
        ? await commands.findFilesByEdition(hit.edition_id).catch(() => null)
        : null
    const files = filesRes && filesRes.status === 'ok' ? filesRes.data : null
    const { editionId } = editionForHit(hit, files)
    if (!editionId) return
    openPeek(editionId)
  } finally {
    activating = false
  }
}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_role_supports_aria_props_implicit -->
<article
  class="cover-card"
  style:--dominant={bg}
  aria-label={hit.title}
  aria-haspopup="dialog"
  onclick={onActivate}
  {@attach attachAsButton}
>
  {#if badge}
    <div class="card-badge">
      {@render badge(hit)}
    </div>
  {/if}
  <div class="cover" aria-hidden="true">
    {#if hit.cover_url}
      <img
        src={hit.cover_url}
        alt=""
        loading="lazy"
        referrerpolicy="no-referrer"
      />
    {:else if coverSrc}
      <img
        src={coverSrc}
        alt=""
        class="cover-fade-in"
      />
    {:else}
      <span class="cover-letter">{letter}</span>
    {/if}
  </div>
  <div class="cover-overlay">
    <div class="cover-title" aria-hidden="true">{hit.title}</div>
    {#if authorsLine}
      <div class="cover-authors">{authorsLine}</div>
    {/if}
  </div>
  {#if actions}
    <div class="card-actions">
      {@render actions(hit)}
    </div>
  {:else if hit.digital_inventory_id && onremove}
    <div class="card-actions">
      <wa-icon-button
        name="trash"
        label="Remove book"
        class="delete-icon"
        onclick={(e: Event) => {
          e.stopPropagation()
          onremove(hit.digital_inventory_id!)
        }}
      ></wa-icon-button>
    </div>
  {/if}
</article>

<style>
  .cover-card {
    display: block;
    position: relative;
    border-radius: var(--wa-border-radius-m, 6px);
    overflow: hidden;
    isolation: isolate;
    cursor: pointer;
    outline: none;
  }

  .cover-card:focus-visible {
    box-shadow: 0 0 0 2px var(--wa-color-focus, currentColor);
  }

  .cover {
    aspect-ratio: 2 / 3;
    background: var(--dominant);
    display: grid;
    place-items: center;
    box-shadow:
      inset 0 0 0 1px rgba(0, 0, 0, 0.08),
      0 1px 3px rgba(0, 0, 0, 0.15);
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .cover-fade-in {
    animation: cover-load 300ms ease;
  }

  @keyframes cover-load {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .cover-letter {
    font-size: clamp(2.5rem, 8vw, 4.5rem);
    font-weight: 600;
    color: rgba(255, 255, 255, 0.92);
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    font-family: var(--wa-font-family-heading, inherit);
  }

  .cover-overlay {
    position: absolute;
    inset: auto 0 0 0;
    padding: 0.75rem 0.875rem;
    color: white;
    background: linear-gradient(
      to top,
      rgba(0, 0, 0, 0.78),
      rgba(0, 0, 0, 0) 100%
    );
    opacity: 0;
    transition: opacity 150ms ease;
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .cover-card:hover .cover-overlay,
  .cover-card:focus-visible .cover-overlay,
  .cover-card:focus-within .cover-overlay {
    opacity: 1;
  }

  .cover-title {
    font-size: 0.9375rem;
    font-weight: 600;
    line-height: 1.2;
  }

  .cover-authors {
    font-size: 0.8125rem;
    opacity: 0.85;
    line-height: 1.2;
  }

  .card-badge {
    position: absolute;
    top: 0.375rem;
    left: 0.375rem;
    z-index: 1;
  }

  .card-actions {
    position: absolute;
    bottom: 0.5rem;
    right: 0.5rem;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .cover-card:hover .card-actions,
  .cover-card:focus-visible .card-actions,
  .cover-card:focus-within .card-actions {
    opacity: 1;
  }

  .delete-icon::part(base) {
    color: white;
    background: rgba(255, 59, 48, 0.85);
    border-radius: 50%;
  }

  .delete-icon::part(base):hover {
    background: rgba(255, 59, 48, 1);
  }
</style>
