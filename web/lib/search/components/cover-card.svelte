<script lang="ts">
import { attachAsButton } from '$lib/a11y/attachments'
import { commands } from '$lib/bindings'
import { editionForHit } from '$lib/catalog/edition-for-hit'
import { openPeek } from '$lib/catalog/peek-state.svelte'
import { coverLetter, dominantColorFor } from '../cover-art'
import type { SearchHit } from '../types'

interface Props {
  hit: SearchHit
}

let { hit }: Props = $props()

const bg = $derived(dominantColorFor(hit.title))
const letter = $derived(coverLetter(hit.title))
const authorsLine = $derived(hit.authors.length === 0 ? '' : hit.authors.join(', '))

let activating = $state(false)

async function onActivate(): Promise<void> {
  if (activating) return
  activating = true
  try {
    // Prefer the digital_inventory lookup: an edition can carry
    // multiple files and only the inventory row tells us which
    // edition is the canonical "view detail" target for this
    // search hit. If the lookup errors out (no row, transient DB
    // hiccup, …) we fall back to the hit's own edition_id so the
    // user is never locked out of detail view.
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
<!--   attachAsButton adds a keydown listener at runtime; Svelte's static
       analyzer cannot see it. role="button" + tabindex="0" set by the
       attachment silence a11y_no_noninteractive_tabindex and
       a11y_no_static_element_interactions on their own. -->
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
  <div class="cover" aria-hidden="true">
    {#if hit.cover_url}
      <img
        src={hit.cover_url}
        alt=""
        loading="lazy"
        referrerpolicy="no-referrer"
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
</style>