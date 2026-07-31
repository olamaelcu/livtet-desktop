<script lang="ts">
import { toast } from 'svelte-sonner'
import { attachAsButton } from '$lib/a11y/attachments'
import { commands, type ImportRequest } from '$lib/bindings'
import { editionForHit } from '$lib/catalog/edition-for-hit'
import { openPeek } from '$lib/catalog/peek-state.svelte'
import { coverLetter, dominantColorFor } from '../cover-art'
import { consumeCoverRefresh, refreshState } from '../cover-refresh.svelte'
import type { SearchHit } from '../types'

interface Props {
  hit: SearchHit
  onremove?: (digitalInventoryId: string) => void
}

let { hit, onremove }: Props = $props()

const bg = $derived(hit.dominant_color ?? dominantColorFor(hit.title))
const letter = $derived(coverLetter(hit.title))
const authorsLine = $derived(hit.authors.length === 0 ? '' : hit.authors.join(', '))
const isRemote = $derived(!hit.edition_id)

let activating = $state(false)
let importing = $state(false)
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
  if (!hit.edition_id || isRemote) return
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

async function onImport(e: Event): Promise<void> {
  e.stopPropagation()
  if (importing) return
  importing = true
  try {
    const request: ImportRequest = {
      title: hit.title,
      authors: hit.authors,
      isbn: hit.isbn,
      isbn_13: hit.isbn_13,
      publisher: hit.publisher,
      page_count: hit.page_count,
      language: hit.language,
      published_date: hit.published_date,
      description: hit.description,
      provider: hit.source,
      provider_work_id: hit.work_id,
      provider_edition_url: null,
    }
    const res = await commands.importEdition(request)
    if (res.status === 'error') {
      toast.error(res.error)
    } else if (res.data === 'AlreadyExists') {
      toast.warning('Already in your catalog')
    } else {
      toast.success(`Imported "${request.title}" to your catalog`)
      openPeek(res.data.Created.edition_id)
    }
  } catch (e) {
    toast.error(e instanceof Error ? e.message : 'Import failed')
  } finally {
    importing = false
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
  {#if isRemote}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!--   wa-button is interactive (button semantics + native Enter/Space) -->
    <div class="import-action">
      <wa-button
        size="s"
        appearance="outlined"
        disabled={importing}
        onclick={onImport}
      >
        {importing ? "…" : "Import"}
      </wa-button>
    </div>
  {:else if hit.digital_inventory_id && onremove}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="delete-action">
      <wa-icon-button
        name="trash"
        label="Remove book"
        class="delete-icon"
        onclick={(e: Event) => {
          e.stopPropagation()
          onremove(hit.digital_inventory_id!)
        }}
      />
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

  .import-action,
  .delete-action {
    position: absolute;
    bottom: 0.5rem;
    right: 0.5rem;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .cover-card:hover .import-action,
  .cover-card:focus-visible .import-action,
  .cover-card:focus-within .import-action,
  .cover-card:hover .delete-action,
  .cover-card:focus-visible .delete-action,
  .cover-card:focus-within .delete-action {
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
