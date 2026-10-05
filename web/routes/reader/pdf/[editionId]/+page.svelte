<script lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import type { PDFDocumentLoadingTask, PDFDocumentProxy } from 'pdfjs-dist'
import * as pdfjs from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { onDestroy, onMount } from 'svelte'
import { page } from '$app/state'
import ActionButton from '$lib/components/ActionButton.svelte'
import { createLoadSession } from '$lib/reader/loadSession'
import { canvasMetrics, clampPage, createRenderGate, fitToWidthScale } from '$lib/reader/pdf'
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

async function goTo(target: number) {
  const next = clampPage(target, pageCount)
  if (next === pageNumber) return
  pageNumber = next
  try {
    await render()
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

  <!-- Pane grows this; the rendered page scrolls inside it. -->
  <div class="viewport" data-scroll-region bind:this={container}>
    <canvas bind:this={canvas}></canvas>
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

  .viewport {
    min-width: 0;
    min-height: 0;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    width: 100%;
    overflow: auto;
    padding: var(--wa-space-m);
    background-color: var(--wa-color-surface-lowered, #f4f4f5);
  }

  canvas {
    display: block;
    max-width: 100%;
    box-shadow: var(--wa-shadow-m, 0 1px 4px rgb(0 0 0 / 0.2));
  }
</style>
