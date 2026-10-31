<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener'
import { toast } from 'svelte-sonner'
import type { IdentifierRef } from '../bindings'
import { commands } from '../bindings'
import ActionButton from '../components/ActionButton.svelte'
import AnnotationList from '../components/AnnotationList.svelte'
import { sanitizeHtml } from '../html'

import { atprotoKeys, catalogKeys, searchKeys } from '../query/keys'
import { openReader as openAudiobookReader } from '../reader'
import { loadAnnotations } from '../reader/progress'
import { canOpenInReader, openReaderWindow } from '../reader/read'
import { coverUrlFor, loadEditionDetail } from '../search'
import ConfirmDialog from './ConfirmDialog.svelte'
import {
  applyEnrichment,
  type EnrichMode,
  type EnrichmentPreview,
  enrichDisabledReason,
  previewEnrichment,
  previewIsEmpty,
  reportEnrichmentError,
  summaryMessage,
} from './enrich'
import { describeCatalogError, formatIdentifier } from './format'
import IdentifierEditDialog from './IdentifierEditDialog.svelte'
import { relinkEditionFile } from './import'
import ReviewInsert from './ReviewInsert.svelte'
import ReviewsDrawer from './ReviewsDrawer.svelte'
import ShelfSelection from './ShelfSelection.svelte'

interface Props {
  editionId: string
  open: boolean
  onclose: () => void
}

let { editionId, open, onclose }: Props = $props()

const detail = createQuery(() => ({
  queryKey: catalogKeys.editionDetail(editionId ?? ''),
  queryFn: async () => (editionId ? loadEditionDetail(editionId) : null),
  enabled: open && editionId !== null,
}))

const book = $derived(detail.data ?? null)

/**
 * Reader annotations live in `reading_progress`, not in the catalog record, so
 * they are a second query against the same edition.
 */
const readerNotes = createQuery(() => ({
  queryKey: catalogKeys.editionReaderNotes(editionId ?? ''),
  queryFn: async () => (editionId ? loadAnnotations(editionId) : []),
  enabled: open && editionId !== null,
}))
const coverUrl = $derived(coverUrlFor(book?.file?.cover_path))
let failedCoverId = $state<string | null>(null)
const showCover = $derived(coverUrl !== undefined && failedCoverId !== book?.id)

const queryClient = useQueryClient()
const FILE_FILTERS = [{ name: 'Books', extensions: ['epub', 'azw3', 'azw', 'pdf', 'm4b', 'm4a'] }]
let relinking = $state(false)
let listening = $state(false)
let selectedShelf = $state<string>('to-read')
let showReviewDialog = $state(false)
let showReviewsDrawer = $state(false)
/** The identifier row the edit dialog is open on, or `null` when it is shut. */
let activeIdentifier = $state<IdentifierRef | null>(null)
let showIdentifierDialog = $state(false)
/** The identifier awaiting confirmation, or `null` when nothing is being removed. */
let pendingRemoval = $state<IdentifierRef | null>(null)

/**
 * Whether a PDS account is signed in, asked only while the review form is
 * open: it decides the publish switch's default and whether it can be used.
 */
const atprotoStatus = createQuery(() => ({
  queryKey: atprotoKeys.status(),
  queryFn: async () => {
    const result = await commands.atprotoStatus()
    // `null` is the signed-out answer, not a failure; only a real error throws.
    if (result.status === 'error') throw new Error(JSON.stringify(result.error))
    return result.data
  },
  enabled: showReviewDialog || showReviewsDrawer,
}))
// Tri-state on purpose: `undefined` until the status query answers, so the
// form's switch does not flash off (and latch off) while it is loading.
const signedIn = $derived(
  atprotoStatus.data === undefined ? undefined : atprotoStatus.data !== null,
)
let reviewForm = $state<{ ok?: boolean; error?: string }>()
let reviewSubmitting = $state(false)

const ENRICH_MODES: { mode: EnrichMode; label: string; hint: string }[] = [
  { mode: 'fill', label: 'Fill gaps', hint: 'Only adds fields this book is missing.' },
  {
    mode: 'union',
    label: 'Union',
    hint: 'Keeps what is here and appends missing list entries.',
  },
  {
    mode: 'overwrite',
    label: 'Overwrite',
    hint: 'Takes the source value wherever the two differ.',
  },
]

/**
 * The previewed diff, shown as a review screen in place of the record.
 * `null` means the body is back on the detail view.
 */
let enrichPreview = $state<EnrichmentPreview | null>(null)
/** How apply will write the preview; `union` keeps the most and loses nothing. */
let enrichMode = $state<EnrichMode>('union')
let previewing = $state(false)
let applying = $state(false)

/** Why enrichment cannot run on this edition, or `null` when it can. */
const enrichBlocked = $derived(
  detail.isPending ? 'Loading this book…' : enrichDisabledReason(book?.identifiers),
)
/** Which side of a conflict the selected mode writes. */
const winningSide = $derived<'current' | 'incoming'>(
  enrichMode === 'overwrite' ? 'incoming' : 'current',
)
const enrichModeHint = $derived(
  ENRICH_MODES.find((option) => option.mode === enrichMode)?.hint ?? '',
)
// `ActionButton` forwards undeclared attributes, which is the only way a
// disabled `wa-button` can explain itself: its own `title` prop is not typed.
const enrichTitle = $derived({ title: enrichBlocked ?? undefined })

/**
 * Fetch the diff and switch the drawer to the review screen. A source that
 * answered with nothing keeps the record on screen instead of opening an empty
 * review.
 */
async function triggerEnrich() {
  if (!editionId || previewing || applying || enrichBlocked !== null) return
  try {
    previewing = true
    const next = await previewEnrichment(editionId)
    if (previewIsEmpty(next)) {
      enrichPreview = null
      toast.info('Nothing new — the installed sources found no changes for this book')
      return
    }
    enrichPreview = next
  } catch (error) {
    reportEnrichmentError(error)
  } finally {
    previewing = false
  }
}

/** Drop the preview without writing anything. */
function cancelEnrich() {
  enrichPreview = null
  enrichMode = 'union'
}

/**
 * Write the preview under the selected mode. The diff is classification only,
 * so the summary, not the preview, decides what the catalog and the search
 * index now hold.
 */
async function applyEnrich() {
  if (!editionId || applying || !enrichPreview) return
  try {
    applying = true
    const summary = await applyEnrichment(editionId, enrichMode)
    enrichPreview = null
    enrichMode = 'union'
    void queryClient.invalidateQueries({ queryKey: catalogKeys.editionDetail(editionId) })
    // A stored cover rewrites `digital_inventory`, which the cover cache and
    // the catalog list both read.
    if (summary.cover_stored) {
      void queryClient.invalidateQueries({ queryKey: catalogKeys.all })
    }
    const changed = summary.fields_added + summary.fields_overwritten + summary.fields_unioned
    if (changed > 0) {
      void queryClient.invalidateQueries({ queryKey: searchKeys.all })
    }
    toast.success(summaryMessage(summary))
  } catch (error) {
    // The preview is kept: a retry is still possible unless it expired, and
    // the backend says so in the message.
    reportEnrichmentError(error)
  } finally {
    applying = false
  }
}
/** The reviewed book's page in the Livtet web app, once publishing resolved it. */
let reviewPageUrl = $state<string | null>(null)
/**
 * How many reviews `ReviewsDrawer` has stored for this edition. Bound from the
 * drawer so the trigger button can show it without fetching on its own.
 */
let localReviewCount = $state(0)
// The drawer is reused across books; a page link from the previous edition
// must not linger on the next one.
$effect(() => {
  void editionId
  reviewPageUrl = null
})

// Same reuse, and the drawer is only hidden rather than unmounted: a preview
// from another book, or from before it was closed, must never come back.
$effect(() => {
  void editionId
  void open
  enrichPreview = null
  enrichMode = 'union'
})

/**
 * Save the review locally, then publish it to the PDS in the background.
 *
 * The local write is the promise the reader gets: publishing is best effort,
 * because a signed-out account or an unreachable index must not cost them the
 * words they just typed.
 */
async function submitReview(text: string, rating: number, publish: boolean) {
  if (!editionId || reviewSubmitting) return
  const title = book?.title ?? ''
  const stars = rating > 0 ? rating : null
  try {
    reviewSubmitting = true
    reviewForm = undefined
    const saved = await commands.saveLocalReview(editionId, text, stars)
    if (saved.status === 'error') {
      reviewForm = { error: saved.error.message }
      return
    }
    reviewForm = { ok: true }
    showReviewDialog = false
    void queryClient.invalidateQueries({ queryKey: catalogKeys.editionReviews(editionId) })
    if (!publish) {
      toast.success('Review saved — not published to your PDS')
      return
    }
    toast.success('Review saved')
    // Publishing is best effort: the local row is already written, so a
    // failure here downgrades to a warning instead of losing the review.
    void commands
      .putLocalReviewToPds(saved.data.id, title)
      .then((sync) => {
        const pageUrl = sync.status === 'ok' ? sync.data.book_page_url : null
        if (pageUrl) reviewPageUrl = pageUrl
        if (sync.status === 'error') {
          toast.error(`Saved locally — publishing failed: ${sync.error.message}`)
        } else if (sync.data.error) {
          toast.error(`Saved locally — publishing failed: ${sync.data.error}`)
        } else {
          toast.success('Review published to your PDS', {
            action: pageUrl
              ? { label: 'View on Livtet', onClick: () => openPage(pageUrl) }
              : undefined,
          })
        }
      })
      .catch((error: unknown) => {
        toast.error(
          `Saved locally — publishing failed: ${error instanceof Error ? error.message : String(error)}`,
        )
      })
  } catch (error) {
    reviewForm = { error: error instanceof Error ? error.message : 'Could not save the review' }
  } finally {
    reviewSubmitting = false
  }
}

function openPage(url: string) {
  void openUrl(url).catch((error: unknown) => {
    toast.error(error instanceof Error ? error.message : 'Could not open the page')
  })
}

const isAudiobook = $derived(book?.format?.toLowerCase() === 'audiobook')
const canListen = $derived(
  isAudiobook && book?.file?.file_path != null && book?.file?.file_status !== 'missing',
)

async function listenBook() {
  if (!editionId || listening) return
  try {
    listening = true
    await openAudiobookReader(editionId)
    onclose()
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not open the reader')
  } finally {
    listening = false
  }
}

async function relinkFile() {
  if (!editionId || relinking) return
  try {
    const picked = await openDialog({ multiple: false, filters: FILE_FILTERS })
    if (!picked || Array.isArray(picked)) return
    relinking = true
    await relinkEditionFile(editionId, picked)
    await queryClient.invalidateQueries({ queryKey: catalogKeys.editionDetail(editionId) })
    toast.success('File re-linked')
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not re-link the file')
  } finally {
    relinking = false
  }
}

function openIdentifierDialog(identifier: IdentifierRef) {
  activeIdentifier = identifier
  showIdentifierDialog = true
}

/**
 * An identifier edit changes this record and the search document built from it
 * (`identifier_values` is searchable), so both caches go stale together.
 */
function refreshAfterIdentifierChange() {
  if (!editionId) return
  void queryClient.invalidateQueries({ queryKey: catalogKeys.editionDetail(editionId) })
  void queryClient.invalidateQueries({ queryKey: searchKeys.all })
}

/**
 * Remove the identifier the row's trash button asked about. The dialog's own
 * Delete does not come through here: it is already an explicit choice inside a
 * modal, while a table row's icon is one stray click away.
 */
async function removeIdentifier() {
  const target = pendingRemoval
  pendingRemoval = null
  if (!target || !editionId) return
  const result = await commands.removeEditionIdentifier(editionId, target.kind, target.value)
  if (result.status === 'error') {
    toast.error(describeCatalogError(result.error))
    return
  }
  toast.success('Identifier removed')
  refreshAfterIdentifierChange()
}

async function readBook() {
  const file = book?.file ?? null
  console.debug('[reader] Read action clicked', {
    editionId,
    readable: canOpenInReader(file),
    fileStatus: file?.file_status ?? null,
  })
  if (!editionId) {
    console.warn('[reader] Read action ignored without an edition id')
    return
  }
  try {
    await openReaderWindow(editionId)
  } catch (error) {
    console.error('[reader] Read action failed', { editionId, error })
    toast.error(error instanceof Error ? error.message : 'Could not open the reader')
  }
}

async function revealFile(path: string) {
  try {
    await revealItemInDir(path)
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not reveal the file')
  }
}
</script>

<!--
  `wa-after-hide` bubbles, and the drawer contains components that dispatch it
  too — a shelf dropdown closes its menu with the same event. Only the drawer's
  own hide may close it, or selecting a menu item would shut the whole drawer.
-->
<wa-drawer
  class="drawer"
  placement="end"
  {open}
  onwa-after-hide={(event) => {
    if (event.target === event.currentTarget) onclose()
  }}
>
  <wa-button-group slot="header-actions">
    {@const file = book?.file}
    {@const path = file?.file_path}
    <ActionButton
      {...enrichTitle}
      onclick={() => triggerEnrich()}
      disabled={enrichBlocked !== null || previewing}
    >
      <wa-icon name={previewing ? 'loader-circle' : 'square-sparkles'}></wa-icon>
      {previewing ? 'Enriching…' : 'Enrich'}
    </ActionButton>
    {#if file?.file_status !== 'ok'}
      <ActionButton onclick={relinkFile} disabled={relinking}>
        <wa-icon name="external-link"></wa-icon>
        Relink
      </ActionButton>
    {:else}
      <ActionButton onclick={() => path && revealFile(path)} disabled={!path}>
        <wa-icon name="folder-open"></wa-icon>
        Open
      </ActionButton>
    {/if}

    {#if canListen}
      <div class="listen">
        <ActionButton onclick={listenBook} disabled={listening}>
          <wa-icon name="play"></wa-icon>
          Listen
        </ActionButton>
      </div>
    {:else}
      <ActionButton onclick={readBook} disabled={!canOpenInReader(book?.file)}>
        <wa-icon name="book-open"></wa-icon>
        Read
      </ActionButton>

    {/if}


  </wa-button-group>
  <div class="body">
    {#if enrichPreview}
      <div class="enrich-review">
        <header class="enrich-head">
          <h2 class="title">Enrichment preview</h2>
          <p class="muted">
            Nothing is written until you apply. The mode decides which side of a conflict wins.
          </p>
        </header>

        <section class="section">
          <h3 class="section-title">Sources</h3>
          <ul class="chips">
            {#each enrichPreview.sources as source (source)}
              <li><wa-badge>{source}</wa-badge></li>
            {/each}
          </ul>
        </section>

        <section class="section">
          <h3 class="section-title">Added</h3>
          {#if enrichPreview.added.length > 0}
            <ul class="changes">
              {#each enrichPreview.added as change (change.field)}
                <li class="change">
                  <span class="change-field">{change.field}</span>
                  <span class="change-value">{change.value}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty">Nothing new.</p>
          {/if}
        </section>

        <section class="section">
          <h3 class="section-title">Unioned</h3>
          {#if enrichPreview.unioned.length > 0}
            <ul class="changes">
              {#each enrichPreview.unioned as addition (addition.field)}
                <li class="change">
                  <span class="change-field">{addition.field}</span>
                  <span class="change-value">{addition.values.join(', ')}</span>
                </li>
              {/each}
            </ul>
            {#if enrichMode === 'fill'}
              <p class="muted">Fill gaps leaves existing lists untouched.</p>
            {/if}
          {:else}
            <p class="empty">No lists to extend.</p>
          {/if}
        </section>

        <section class="section">
          <h3 class="section-title">Conflicts</h3>
          {#if enrichPreview.conflicts.length > 0}
            <p class="muted">
              {winningSide === 'incoming'
                ? 'Overwrite takes the incoming value.'
                : 'Your current value is kept; the incoming one is shown next to it.'}
            </p>
            <ul class="changes">
              {#each enrichPreview.conflicts as conflict (conflict.field)}
                <li class="conflict">
                  <span class="change-field">{conflict.field}</span>
                  <span
                    class="conflict-side"
                    data-side="current"
                    class:wins={winningSide === 'current'}
                    class:loses={winningSide !== 'current'}>{conflict.current}</span
                  >
                  <span
                    class="conflict-side"
                    data-side="incoming"
                    class:wins={winningSide === 'incoming'}
                    class:loses={winningSide !== 'incoming'}>{conflict.incoming}</span
                  >
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty">No conflicts.</p>
          {/if}
        </section>

        <section class="section">
          <h3 class="section-title">Found identifiers</h3>
          {#if enrichPreview.found.length > 0}
            <ul class="identifiers">
              {#each enrichPreview.found as identifier (`${identifier.kind}:${identifier.value}`)}
                <li class="identifier">
                  <wa-badge>{identifier.kind}</wa-badge>
                  <span class="mono">{formatIdentifier(identifier.value)}</span>
                </li>
              {/each}
            </ul>
            <p class="muted">Saved whichever mode you pick.</p>
          {:else}
            <p class="empty">No new identifiers.</p>
          {/if}
        </section>

        <section class="section">
          <h3 class="section-title">Cover</h3>
          {#if enrichPreview.cover}
            <p>
              {enrichPreview.cover.mime}
              {enrichPreview.cover.storable ? '— will be saved' : '— cannot be stored'}
            </p>
          {:else}
            <p class="empty">No cover offered.</p>
          {/if}
        </section>

        <section class="section">
          <h3 class="section-title">Mode</h3>
          <div class="mode-control" role="group" aria-label="Enrichment mode">
            {#each ENRICH_MODES as option (option.mode)}
              <ActionButton
                aria-pressed={enrichMode === option.mode}
                variant={enrichMode === option.mode ? 'brand' : undefined}
                disabled={applying}
                onclick={() => (enrichMode = option.mode)}
              >
                {option.label}
              </ActionButton>
            {/each}
          </div>
          <p class="muted">{enrichModeHint}</p>
        </section>

        <div class="review-actions">
          <ActionButton onclick={applyEnrich} disabled={applying}>
            {applying ? 'Applying…' : 'Apply'}
          </ActionButton>
          <ActionButton onclick={cancelEnrich} disabled={applying}>Cancel</ActionButton>
        </div>
      </div>
    {:else if detail.isPending}
      <p class="muted">Loading…</p>
    {:else if detail.isError}
      <p class="error">Could not load this book's details.</p>
    {:else if book}
      <header class="header">
        <figure class="cover">
          {#if showCover}
            <img
              src={coverUrl}
              alt={book.title ?? 'Cover'}
              onerror={() => (failedCoverId = book?.id ?? '')}
            />
          {:else}
            <figcaption class="detail-placeholder">{book.title ?? 'Untitled'}</figcaption>
          {/if}
        </figure>
        <div class="heading">
          <h2 class="title">{book.title ?? 'Untitled'}</h2>
          {#if book.authors.length > 0}
            <ul class="authors">
              {#each book.authors as author (author.name + author.role)}
                <li class="author">
                  <span>{author.name}</span>
                  <wa-badge title={author.role}>{author.role_label}</wa-badge>
                </li>
              {/each}
            </ul>
          {/if}
          <dl class="facts">
            {#if book.format}
              <div class="fact"><dt>Format</dt><dd>{book.format}</dd></div>
            {/if}
            {#if book.published_date}
              <div class="fact"><dt>Published</dt><dd>{book.published_date}</dd></div>
            {/if}
            {#if book.language_name}
              <div class="fact"><dt>Language</dt><dd>{book.language_name}</dd></div>
            {/if}
            {#if book.publishers.length > 0}
              <div class="fact"><dt>Publisher</dt><dd>{book.publishers.join(', ')}</dd></div>
            {/if}
          </dl>
        </div>
      </header>

      <section class="section">
        <h3 class="section-title">Shelf</h3>
        <ShelfSelection
          {editionId}
          shelfKey={selectedShelf}
          bookTitle={book.title ?? ''}
          onChange={(k) => (selectedShelf = k)}
        />
      </section>

      {#if book.file?.file_status === 'missing'}
        <p class="error">The linked file is missing — the original was moved or deleted.</p>
      {/if}

      {#if book.identifiers.length > 0}
        <section class="section">
          <h3 class="section-title">Identifiers</h3>
          <table class="identifier-table">
            <thead>
              <tr>
                <th scope="col">Kind</th>
                <th scope="col">Value</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each book.identifiers as identifier (identifier.value)}
                <tr>
                  <td class="identifier-kind">{identifier.kind}</td>
                  <td class="mono">
                    <button
                      type="button"
                      class="identifier-value"
                      title={identifier.value}
                      onclick={() => openIdentifierDialog(identifier)}
                    >
                      {formatIdentifier(identifier.value)}
                    </button>
                  </td>
                  <td class="identifier-actions">
                    <button
                      type="button"
                      class="identifier-action"
                      title="Edit identifier"
                      aria-label="Edit {identifier.kind} {identifier.value}"
                      onclick={() => openIdentifierDialog(identifier)}
                    >
                      <wa-icon name="square-pen"></wa-icon>
                    </button>
                    <button
                      type="button"
                      class="identifier-action is-remove"
                      title="Remove identifier"
                      aria-label="Remove {identifier.kind} {identifier.value}"
                      onclick={() => (pendingRemoval = identifier)}
                    >
                      <wa-icon name="trash"></wa-icon>
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </section>
      {/if}

      {#if book.description}
        <section class="section">
          <h3 class="section-title">Description</h3>
          <p>{@html sanitizeHtml(book.description)}</p>
        </section>
      {/if}

      <section class="section" id="review-section">
        <h3 class="section-title">Write a Review</h3>
        <div class="review-actions">
          <ActionButton onclick={() => (showReviewDialog = true)}>
            <wa-icon name="square-pen"></wa-icon>
            Write Review
          </ActionButton>
          <ActionButton onclick={() => (showReviewsDrawer = true)}>
            <wa-icon name="comments"></wa-icon>
            Reviews{localReviewCount > 0 ? ` (${localReviewCount})` : ''}
          </ActionButton>
          {#if reviewPageUrl}
            <ActionButton onclick={() => openPage(reviewPageUrl!)}>
              <wa-icon name="arrow-up-right-from-square"></wa-icon>
              View on Livtet
            </ActionButton>
          {/if}
        </div>
      </section>

      <wa-dialog
        open={showReviewDialog}
        label="Add new review"
        onwa-after-hide={() => (showReviewDialog = false)}
      >
        <ReviewInsert
          form={reviewForm}
          {signedIn}
          onSubmit={submitReview}
          onClose={() => (showReviewDialog = false)}
        />
      </wa-dialog>

      <wa-dialog
        open={showIdentifierDialog}
        label="Edit identifier"
        onwa-after-hide={() => (showIdentifierDialog = false)}
      >
        <IdentifierEditDialog
          {editionId}
          identifier={activeIdentifier}
          open={showIdentifierDialog}
          onSaved={refreshAfterIdentifierChange}
          onClose={() => (showIdentifierDialog = false)}
        />
      </wa-dialog>

      <ConfirmDialog
        open={pendingRemoval !== null}
        title="Remove identifier"
        message={pendingRemoval
          ? `Remove ${pendingRemoval.kind} ${pendingRemoval.value} from this book? The catalog entry stops carrying it.`
          : ''}
        confirmLabel="Remove"
        onconfirm={removeIdentifier}
        oncancel={() => (pendingRemoval = null)}
      />


      {#if readerNotes.data && readerNotes.data.length > 0}
        <section class="section">
          <AnnotationList notes={readerNotes.data} title="Reader notes" />
        </section>
      {/if}

      {#if book.notes}
        <section class="section">
          <h3 class="section-title">Catalog notes</h3>
          <p>{book.notes}</p>
        </section>
      {/if}
    {:else}
      <p class="muted">No catalog record found for this book.</p>
    {/if}
  </div>
</wa-drawer>

<ReviewsDrawer
  {editionId}
  open={showReviewsDrawer}
  enabled={open && editionId !== null}
  bind:count={localReviewCount}
  bookTitle={book?.title ?? ''}
  {signedIn}
  onclose={() => (showReviewsDrawer = false)}
/>

<style>
.body {
display: flex;
flex-direction: column;
gap: var(--wa-space-l);
}

.drawer {
/* `wa-drawer` sets `width: var(--size)`. */
--size: 90vw;
}

/* Cover left, everything else in one column beside it: the facts are part of
   the record's identity, not a separate list below the fold. */
.header {
display: grid;
grid-template-columns: 12rem minmax(0, 1fr);
gap: var(--wa-space-l);
align-items: start;
}

.cover {
width: 100%;
aspect-ratio: 2 / 3;
margin: 0;
overflow: hidden;
border: 0.0625rem solid var(--wa-color-surface-border);
border-radius: var(--wa-border-radius);
background: var(--wa-color-fill-quiet);
}

.cover img {
width: 100%;
height: 100%;
object-fit: cover;
display: block;
}

.detail-placeholder {
display: flex;
align-items: center;
justify-content: center;
height: 100%;
padding: var(--wa-space-s);
font-size: var(--wa-font-size-xs);
color: var(--wa-color-text-quiet);
text-align: center;
}

.heading {
display: flex;
flex-direction: column;
gap: var(--wa-space-s);
min-width: 0;
}

.title {
margin: 0;
font-family: var(--wa-font-family-heading);
font-size: 1.55rem;
font-weight: 500;
line-height: 1.15;
text-wrap: balance;
}

.authors {
display: flex;
flex-direction: column;
gap: var(--wa-space-3xs);
margin: 0;
padding: 0;
list-style: none;
}

.author {
display: flex;
align-items: center;
gap: var(--wa-space-xs);
}

.facts {
display: grid;
grid-template-columns: repeat(2, minmax(0, 1fr));
gap: var(--wa-space-s) var(--wa-space-l);
margin: 0;
padding-top: var(--wa-space-3xs);
}

.fact {
display: flex;
flex-direction: column;
gap: 0.1rem;
min-width: 0;
}

.fact dt {
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-2xs, 0.7rem);
font-weight: 600;
letter-spacing: 0.06em;
text-transform: uppercase;
}

.fact dd {
margin: 0;
overflow-wrap: anywhere;
}

/* A narrow drawer cannot hold a 12rem cover and a two-column fact grid. */
@media (max-width: 52rem) {
.header {
grid-template-columns: 8rem minmax(0, 1fr);
}

.facts {
grid-template-columns: minmax(0, 1fr);
}
}

.section {
display: flex;
flex-direction: column;
gap: var(--wa-space-s);
}

.section-title {
margin: 0;
font-size: var(--wa-font-size-s);
color: var(--wa-color-text-quiet);
text-transform: uppercase;
letter-spacing: 0.05em;
}

.section p {
margin: 0;
overflow-wrap: anywhere;
}

.identifier-table {
width: 100%;
border-collapse: collapse;
font-size: var(--wa-font-size-s, 0.9rem);
}

.identifier-table th {
padding: 0 0 var(--wa-space-3xs);
border-bottom: 0.0625rem solid var(--wa-color-surface-border);
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-2xs, 0.7rem);
font-weight: 600;
letter-spacing: 0.06em;
text-align: start;
text-transform: uppercase;
}

.identifier-table td {
padding: var(--wa-space-3xs) var(--wa-space-xs) var(--wa-space-3xs) 0;
border-bottom: 0.0625rem solid var(--wa-color-surface-border);
vertical-align: baseline;
}

.identifier-table tbody tr:hover td {
background: var(--wa-color-fill-quiet);
}

.identifier-kind {
width: 6rem;
color: var(--wa-color-text-quiet);
font-family: var(--wa-font-family-mono, monospace);
font-size: var(--wa-font-size-2xs, 0.7rem);
letter-spacing: 0.04em;
text-transform: uppercase;
}

/* A button so the value is reachable from the keyboard; it reads as text. */
.identifier-value {
padding: 0;
border: 0;
background: none;
color: inherit;
font: inherit;
text-align: start;
cursor: text;
overflow-wrap: anywhere;
}

.identifier-value:hover,
.identifier-value:focus-visible {
color: var(--wa-color-text-normal);
text-decoration: underline;
text-underline-offset: 0.2em;
}

.identifier-actions {
width: 3.5rem;
text-align: end;
white-space: nowrap;
}

/* Dimmed but present, so the affordance is discoverable without hovering. */
.identifier-action {
padding: 0;
border: 0;
border-radius: var(--wa-border-radius);
background: none;
color: var(--wa-color-text-quiet);
cursor: pointer;
opacity: 0.35;
transition: opacity 120ms ease;
}

.identifier-table tbody tr:hover .identifier-action,
.identifier-action:focus-visible {
opacity: 1;
}

.identifier-action:hover {
color: var(--wa-color-text-normal);
}

.identifier-action.is-remove:hover {
color: var(--wa-color-danger);
}

.sr-only {
position: absolute;
width: 1px;
height: 1px;
overflow: hidden;
clip-path: inset(50%);
white-space: nowrap;
}

.mono {
font-family: var(--wa-font-family-mono, monospace);
overflow-wrap: anywhere;
}

.muted {
color: var(--wa-color-text-quiet);
}

.empty {
margin: 0;
padding: var(--wa-space-xs);
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-s);
}

.error {
color: var(--wa-color-danger);
}

.enrich-review {
display: flex;
flex-direction: column;
gap: var(--wa-space-l);
}

.enrich-head {
display: flex;
flex-direction: column;
gap: var(--wa-space-2xs);
}

.enrich-head p {
margin: 0;
}

.chips,
.changes {
display: flex;
flex-direction: column;
gap: var(--wa-space-2xs);
margin: 0;
padding: 0;
list-style: none;
}

.chips {
flex-direction: row;
flex-wrap: wrap;
}

.change,
.conflict {
display: flex;
flex-wrap: wrap;
align-items: center;
gap: var(--wa-space-xs);
}

.change-field {
min-width: 7rem;
color: var(--wa-color-text-quiet);
font-size: var(--wa-font-size-s);
}

.change-value,
.conflict-side {
overflow-wrap: anywhere;
}

.conflict-side {
padding: 0 var(--wa-space-2xs);
border: 1px solid var(--wa-color-border-default);
border-radius: var(--wa-border-radius);
}

/* The same diff reads differently per mode: mark the side apply will write. */
.conflict-side.wins {
background: var(--wa-color-surface-alt);
border-color: var(--wa-color-text-normal);
font-weight: 600;
}

.conflict-side.loses {
opacity: 0.55;
text-decoration: line-through;
}

.mode-control {
display: flex;
flex-wrap: wrap;
gap: var(--wa-space-xs);
}

.review-actions {
display: flex;
flex-wrap: wrap;
gap: var(--wa-space-xs);
margin: 0 auto;
}

</style>
