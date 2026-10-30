<script lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { PDFDocumentLoadingTask, PDFDocumentProxy } from 'pdfjs-dist'
import * as pdfjs from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { onDestroy, onMount } from 'svelte'
import { page } from '$app/state'
import ActionButton from '$lib/components/ActionButton.svelte'
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import type { Bookmark } from '$lib/reader/bookmark'
import { addBookmark, isBookmarked, loadBookmarks, removeBookmark } from '$lib/reader/bookmark'
import { createLoadSession } from '$lib/reader/loadSession'
import { canvasMetrics, clampPage, createRenderGate, fitToWidthScale } from '$lib/reader/pdf'
import type { Annotation } from '$lib/reader/progress'
import { debouncedSave, loadProgress, saveProgress } from '$lib/reader/progress'
import { loadPdfReaderPublication } from '$lib/reader/read'
import ReaderLayout from '../../ReaderLayout.svelte'

// Same-origin worker, bundled by Vite: `worker-src` falls back to
// `child-src 'self'`, so no CSP change is needed for it.
pdfjs.GlobalWorkerOptions.workerSrc = workerUrl

// Staged out of pdfjs-dist by `scripts/stage-pdfjs.mjs`. Without these,
// pdf.js silently renders blank or garbled output for CJK encodings,
// non-embedded standard fonts, and JPEG 2000 scans.
const PDFJS_ASSETS = {
  cMapUrl: '/pdfjs/cmaps/',
  standardFontDataUrl: '/pdfjs/standard_fonts/',
  iccUrl: '/pdfjs/iccs/',
  wasmUrl: '/pdfjs/wasm/',
}

const editionId = $derived(page.params.editionId ?? '')

const session = createLoadSession()
const gate = createRenderGate()

let container = $state<HTMLElement | undefined>(undefined)
let canvas = $state<HTMLCanvasElement | undefined>(undefined)
let pdfDocument = $state<PDFDocumentProxy | undefined>(undefined)
let loadingTask = $state<PDFDocumentLoadingTask | undefined>(undefined)
let phase = $state<'loading' | 'ready' | 'error'>('loading')
let failure = $state('')
let title = $state('Reader')
let pageNumber = $state(1)
let pageCount = $state(0)

// Annotations loaded from the edition's saved progress; appended on demand.
let annotations = $state<Annotation[]>([])
let composerOpen = $state(false)

// Page bookmarks: markers on a page with no note text and no colour.
let bookmarks = $state<Bookmark[]>([])
let bookmarksLoaded = $state(false)

/** Whether a bookmark save is in flight, so a second click cannot duplicate it. */
let bookmarking = $state(false)

/** Whether the page on screen already carries a bookmark. */
const pageBookmarked = $derived(isBookmarked(bookmarks, { kind: 'pdf', pageNumber }))

/** Whether the note cards are revealed over the page by the FAB. */
let notesVisible = $state(false)

/** Whether the note list is open. A jump closes it. */
let panelOpen = $state(false)

// Debounced save for pdf page changes
const savePdfProgress = debouncedSave(
  () => editionId,
  () => ({
    kind: 'pdf' as const,
    pdf: { pageNumber },
    annotations,
  }),
)

function messageOf(error: unknown): string {
  if (error !== null && typeof error === 'object') {
    const name = (error as { name?: unknown }).name
    if (name === 'PasswordException') {
      return 'This PDF is password-protected, so it cannot be opened here.'
    }
    if (name === 'InvalidPDFException') {
      return 'This file is not a readable PDF. Try re-importing it.'
    }
  }
  return error instanceof Error ? error.message : 'Could not open this PDF.'
}

async function render() {
  const instance = pdfDocument
  const target = canvas
  const host = container
  if (!instance || !target || !host) return
  const pdfPage = await instance.getPage(pageNumber)
  const unscaled = pdfPage.getViewport({ scale: 1 })
  // The viewport padding is part of the host's box, so measure the content
  // width rather than the border box.
  const available = host.clientWidth - readerGutter(host)
  const scale = fitToWidthScale(available, unscaled.width)
  const viewport = pdfPage.getViewport({ scale })
  const ratio = window.devicePixelRatio
  const metrics = canvasMetrics({ width: viewport.width, height: viewport.height }, ratio)
  target.width = metrics.pixelWidth
  target.height = metrics.pixelHeight
  target.style.width = `${metrics.cssWidth}px`
  target.style.height = `${metrics.cssHeight}px`
  // Draw at the backing-store resolution so HiDPI text stays sharp.
  const drawScale = metrics.pixelWidth / metrics.cssWidth
  await gate.run(() =>
    pdfPage.render({
      canvas: target,
      viewport,
      transform: drawScale === 1 ? undefined : [drawScale, 0, 0, drawScale, 0, 0],
    }),
  )
}

/** Horizontal padding of the viewport, so fit-to-width accounts for it. */
function readerGutter(host: HTMLElement): number {
  const styles = getComputedStyle(host)
  return Number.parseFloat(styles.paddingLeft) + Number.parseFloat(styles.paddingRight) || 0
}

/// Tear down a loading task and the document it produced.
///
/// `destroy` lives on the loading task, not the proxy, and it aborts the
/// in-flight range requests as well as the worker.
async function teardown(task: PDFDocumentLoadingTask | undefined) {
  if (!task) return
  await task.destroy().catch(() => {})
}

async function load() {
  const generation = session.begin()
  gate.cancel()
  const previous = loadingTask
  loadingTask = undefined
  pdfDocument = undefined
  await teardown(previous)
  if (!session.isCurrent(generation)) return
  phase = 'loading'
  failure = ''
  let task: PDFDocumentLoadingTask | undefined
  try {
    if (!editionId) throw new Error('No edition id was provided to the reader.')
    const publication = await loadPdfReaderPublication(editionId)
    if (!session.isCurrent(generation)) return
    title = publication.title || `Reader — ${editionId}`
    // `disableAutoFetch` is what makes the ranged loopback route pay off:
    // without it pdf.js eagerly pulls the whole file up front.
    task = pdfjs.getDocument({
      url: publication.pdfUrl,
      disableAutoFetch: true,
      ...PDFJS_ASSETS,
    })
    const instance = await task.promise
    if (!session.isCurrent(generation)) {
      await teardown(task)
      return
    }
    loadingTask = task
    pdfDocument = instance
    pageCount = instance.numPages
    // Restore last-read page if one was saved
    const progress = await loadProgress(editionId)
    if (progress?.pdf?.pageNumber !== undefined && progress.pdf.pageNumber <= pageCount) {
      pageNumber = progress.pdf.pageNumber
    }
    if (progress?.annotations) annotations = progress.annotations
    bookmarks = await loadBookmarks(editionId)
    bookmarksLoaded = true
    pageNumber = clampPage(pageNumber, pageCount)
    await render()
    if (!session.isCurrent(generation)) return
    phase = 'ready'
  } catch (error) {
    await teardown(task)
    if (!session.isCurrent(generation)) return
    loadingTask = undefined
    pdfDocument = undefined
    phase = 'error'
    failure = messageOf(error)
  }
}

/**
 * Record the composer draft as a note on the current page.
 *
 * PDF annotations are page-tied: there is no text layer to anchor a span
 * selection to, so the note belongs to whatever page is on screen. WebKitGTK
 * implements no `window.prompt`, so text is collected in an in-page composer.
 */
function saveAnnotation(text: string) {
  const annotation: Annotation = {
    id: crypto.randomUUID(),
    text,
    pageNumber,
  }
  annotations = [...annotations, annotation]
  void saveProgress(editionId, {
    kind: 'pdf',
    pdf: { pageNumber },
    annotations,
  })
  closeComposer()
  notesVisible = true
}

/**
 * Bookmark the page on screen.
 *
 * PDF bookmarks are page-tied, so the page number is the whole position.
 * Bookmarking only ever adds: a bookmarked page has its button disabled, and
 * the panel owns removal.
 */
async function bookmarkCurrentPage() {
  if (!bookmarksLoaded || bookmarking) return
  if (isBookmarked(bookmarks, { kind: 'pdf', pageNumber })) return
  bookmarking = true
  try {
    const added = await addBookmark(editionId, { kind: 'pdf', pageNumber })
    if (added) {
      bookmarks = [...bookmarks, added]
      notesVisible = true
    }
  } finally {
    bookmarking = false
  }
}

/** Move to the page a bookmark points at, then dismiss the list. */
function jumpToBookmark(bookmark: Bookmark) {
  panelOpen = false
  if (bookmark.pageNumber !== undefined) void goTo(bookmark.pageNumber)
}

/** Drop a bookmark without moving the reader. */
async function removeBookmarkEntry(bookmark: Bookmark) {
  if (!(await removeBookmark(bookmark.id))) return
  bookmarks = bookmarks.filter((item) => item.id !== bookmark.id)
}

/**
 * Go to the page a note was written on, then dismiss the list so the page it
 * points at is not hidden behind it.
 */
function jumpTo(note: Annotation) {
  panelOpen = false
  if (note.pageNumber !== undefined) void goTo(note.pageNumber)
}

function openComposer() {
  composerOpen = true
}

function closeComposer() {
  composerOpen = false
}

async function goTo(target: number) {
  const next = clampPage(target, pageCount)
  if (next === pageNumber) return
  pageNumber = next
  try {
    await render()
    // Debounced save after successful navigation
    savePdfProgress()
  } catch (error) {
    phase = 'error'
    failure = messageOf(error)
  }
}

function previous() {
  void goTo(pageNumber - 1)
}

function next() {
  void goTo(pageNumber + 1)
}

async function close() {
  await getCurrentWindow().close()
}

onMount(() => {
  void load()

  // Re-render on resize so fit-to-width keeps holding.
  const host = container
  if (!host || typeof ResizeObserver === 'undefined') return
  let frame = 0
  const observer = new ResizeObserver(() => {
    if (phase !== 'ready') return
    cancelAnimationFrame(frame)
    frame = requestAnimationFrame(() => {
      void render().catch(() => {})
    })
  })
  observer.observe(host)
  return () => {
    cancelAnimationFrame(frame)
    observer.disconnect()
  }
})

onDestroy(() => {
  session.invalidate()
  gate.cancel()
  // Save final page progress before teardown
  void saveProgress(editionId, {
    kind: 'pdf',
    pdf: { pageNumber },
  })
  const task = loadingTask
  loadingTask = undefined
  pdfDocument = undefined
  void teardown(task)
})
</script>

<ReaderLayout>
  <header class="bar">
    <h1 class="title">{title}</h1>
    <div class="controls">
      {#if phase === 'ready'}
        <span class="count">{pageNumber} / {pageCount}</span>
      {/if}
      <ActionButton onclick={previous} disabled={phase !== 'ready' || pageNumber <= 1}>
        <wa-icon name="chevron-left"></wa-icon>
        Previous
      </ActionButton>
      <ActionButton onclick={next} disabled={phase !== 'ready' || pageNumber >= pageCount}>
        Next
        <wa-icon name="chevron-right"></wa-icon>
      </ActionButton>
      <ActionButton onclick={openComposer} disabled={phase !== 'ready'}>
        <wa-icon name="comment"></wa-icon>
        Annotate
      </ActionButton>
      <ActionButton
        onclick={() => void bookmarkCurrentPage()}
        disabled={phase !== 'ready' || !bookmarksLoaded || bookmarking || pageBookmarked}
        variant={pageBookmarked ? 'brand' : undefined}
        aria-pressed={pageBookmarked}
      >
        <wa-icon name="bookmark"></wa-icon>
        {pageBookmarked ? 'Bookmarked' : 'Bookmark'}
      </ActionButton>
      <ActionButton onclick={() => void close()}>Close</ActionButton>
    </div>
  </header>

  {#if phase === 'loading'}
    <p class="muted">Loading&hellip;</p>
  {:else if phase === 'error'}
    <div class="error">
      <p>Could not open this PDF: {failure}</p>
      <ActionButton onclick={() => void load()}>Retry</ActionButton>
    </div>
  {/if}

  {#if composerOpen}
    <AnnotationComposer
      placeholder={`Note text for page ${pageNumber}`}
      onsave={saveAnnotation}
      oncancel={closeComposer}
    />
  {/if}

  <div class="stage">
    <!-- Pane grows this; the rendered page scrolls inside it. -->
    <div class="viewport" data-scroll-region bind:this={container}>
      <canvas bind:this={canvas}></canvas>
    </div>

    {#if (annotations.length > 0 || bookmarks.length > 0) && phase === 'ready'}
      <div class="fab">
        <ActionButton
          variant="brand"
          onclick={() => {
            // A jump closes the list; while the layer is still on, the FAB
            // reopens it instead of turning everything off.
            if (notesVisible && !panelOpen) {
              panelOpen = true
              return
            }
            notesVisible = !notesVisible
            panelOpen = notesVisible
          }}
          aria-pressed={notesVisible}
          aria-haspopup="dialog"
          aria-expanded={panelOpen}
        >
          <wa-icon name="comment-dots"></wa-icon>
          {#if annotations.length > 0}
            Notes ({annotations.length})
          {/if}
          {#if annotations.length > 0 && bookmarks.length > 0}
            ·
          {/if}
          {#if bookmarks.length > 0}
            Bookmarks ({bookmarks.length})
          {/if}
        </ActionButton>
      </div>
    {/if}

    {#if notesVisible && panelOpen && (annotations.length > 0 || bookmarks.length > 0)}
      <div class="notes" role="dialog" aria-label="Notes and bookmarks">
        {#if bookmarks.length > 0}
          <p class="notes-heading">Bookmarks ({bookmarks.length})</p>
          {#each bookmarks as bookmark (bookmark.id)}
            <div class="bookmark-row">
              <button type="button" class="note" onclick={() => jumpToBookmark(bookmark)}>
                {#if bookmark.pageNumber !== undefined}
                  <span class="note-page">Page {bookmark.pageNumber}</span>
                {/if}
              </button>
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <wa-button size="s" onclick={() => void removeBookmarkEntry(bookmark)}>
                <wa-icon name="trash"></wa-icon>
                Remove
              </wa-button>
            </div>
          {/each}
        {/if}
        {#if annotations.length > 0}
          <p class="notes-heading">Notes ({annotations.length})</p>
        {/if}
        {#each annotations as note (note.id)}
          <button type="button" class="note" onclick={() => jumpTo(note)}>
            {#if note.pageNumber !== undefined}
              <span class="note-page">Page {note.pageNumber}</span>
            {/if}
            <span class="note-text">{note.text}</span>
          </button>
        {/each}
      </div>
    {/if}
  </div>

</ReaderLayout>

<style>
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-m);
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-m);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: var(--wa-space-2xs);
  }

  .count {
    font-variant-numeric: tabular-nums;
    color: var(--wa-color-text-secondary);
  }

  .muted {
    padding: 0 var(--wa-space-m);
    color: var(--wa-color-text-secondary);
  }

  .error {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    align-items: flex-start;
    padding: 0 var(--wa-space-m);
    color: var(--wa-color-danger);
  }

  .error p {
    margin: 0;
  }

  /* Positioning context for the FAB and the floating note cards. */
  .stage {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .fab {
    position: absolute;
    inset-block-end: var(--wa-space-m);
    inset-inline-end: var(--wa-space-m);
    z-index: 2;
  }

  /*
    The list floats over the page and never takes layout space. PDF notes are
    page-tied, so each entry names its page and jumps there on click.
  */
  .notes {
    position: absolute;
    z-index: 2;
    inset-block-start: var(--wa-space-m);
    inset-inline-end: var(--wa-space-m);
    width: min(20rem, calc(100% - 2 * var(--wa-space-m)));
    max-height: 60%;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-border-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-border);
    background: var(--wa-color-surface);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
  }

  .notes-heading {
    margin: 0 0 var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs, 0.8125rem);
    color: var(--wa-color-text-secondary);
  }

  /* A note entry: click jumps to the page the note was written on. */
  .note {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    width: 100%;
    padding: var(--wa-space-2xs) 0;
    border: 0;
    border-block-start: 1px solid var(--wa-color-border);
    background: none;
    color: inherit;
    font: inherit;
    font-size: var(--wa-font-size-s);
    text-align: start;
    cursor: pointer;
  }

  .note:hover,
  .note:focus-visible {
    background: var(--wa-color-surface-lowered, rgb(0 0 0 / 0.04));
  }

  .note-page {
    font-size: var(--wa-font-size-2xs, 0.75rem);
    color: var(--wa-color-text-muted);
  }

  .bookmark-row {
    display: flex;
    align-items: center;
    gap: var(--wa-space-2xs);
  }

  .note-text {
    margin: 0;
  }

  .viewport {
    min-width: 0;
    min-height: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    width: 100%;
    padding: var(--wa-space-m);
    overflow-y: auto;
  }

  canvas {
    display: block;
    max-width: 100%;
    box-shadow: var(--wa-shadow-m, 0 1px 4px rgb(0 0 0 / 0.2));
  }
</style>
