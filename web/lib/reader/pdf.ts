// Non-DOM logic for the PDF reader route (ADR 0037).
//
// The route itself owns the canvas and the pdf.js document; everything here is
// arithmetic and sequencing, kept separate so it can be tested without a
// browser — the same split as `loadSession.ts` and `fetcher.ts`.

/** A started render that can be called off. Models pdf.js's `RenderTask`. */
export interface CancellableRender {
  cancel: () => void
  promise: Promise<unknown>
}

/** Width and height of a pdf.js viewport, in CSS pixels. */
export interface PageSize {
  width: number
  height: number
}

export interface CanvasMetrics {
  /** Size of the canvas element, in CSS pixels. */
  cssWidth: number
  cssHeight: number
  /** Size of the canvas backing store, in device pixels. */
  pixelWidth: number
  pixelHeight: number
}

/**
 * Bring `requested` inside `[1, pageCount]`, flooring fractions.
 *
 * A not-yet-loaded document reports no pages; page 1 is still the right answer,
 * because pdf.js rejects page 0.
 */
export function clampPage(requested: number, pageCount: number): number {
  const last = Math.max(1, Math.floor(pageCount))
  if (!Number.isFinite(requested)) return 1
  return Math.min(last, Math.max(1, Math.floor(requested)))
}

/**
 * Scale that fits a page of `unscaledWidth` into `availableWidth`.
 *
 * Returns 1 when either measurement is unusable: a viewport measures 0 before
 * layout settles, and pdf.js throws on a non-positive scale.
 */
export function fitToWidthScale(availableWidth: number, unscaledWidth: number): number {
  if (!Number.isFinite(availableWidth) || !Number.isFinite(unscaledWidth)) return 1
  if (availableWidth <= 0 || unscaledWidth <= 0) return 1
  return availableWidth / unscaledWidth
}

/**
 * Canvas element size and backing-store size for a page at a given ratio.
 *
 * Drawing at the device pixel ratio is what keeps text sharp on HiDPI screens;
 * the element stays at CSS size so layout is unaffected.
 */
export function canvasMetrics(page: PageSize, devicePixelRatio: number): CanvasMetrics {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1
  return {
    cssWidth: page.width,
    cssHeight: page.height,
    pixelWidth: Math.round(page.width * ratio),
    pixelHeight: Math.round(page.height * ratio),
  }
}

/** Whether a rejection is pdf.js calling off a render we cancelled ourselves. */
function isCancellation(error: unknown): boolean {
  if (error === null || typeof error !== 'object') return false
  const name = (error as { name?: unknown }).name
  if (name === 'RenderingCancelledException') return true
  const message = (error as { message?: unknown }).message
  return typeof message === 'string' && message.toLowerCase().includes('cancel')
}

export interface RenderGate {
  /** Start a render, cancelling whatever is still in flight. */
  run: (start: () => CancellableRender) => Promise<void>
  /** Cancel the in-flight render, if any. For teardown. */
  cancel: () => void
}

/**
 * Serialize renders onto a single canvas.
 *
 * Two concurrent `page.render` calls interleave their draws on one context, so
 * a new render must cancel the previous one. The resulting
 * `RenderingCancelledException` is expected control flow and is swallowed; any
 * other failure propagates so the route can show it.
 */
export function createRenderGate(): RenderGate {
  let inFlight: CancellableRender | undefined

  return {
    async run(start) {
      inFlight?.cancel()
      const task = start()
      inFlight = task
      try {
        await task.promise
      } catch (error) {
        if (!isCancellation(error)) throw error
      } finally {
        if (inFlight === task) inFlight = undefined
      }
    },
    cancel() {
      inFlight?.cancel()
      inFlight = undefined
    },
  }
}
