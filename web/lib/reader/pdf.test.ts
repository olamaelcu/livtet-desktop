import { describe, expect, it, vi } from 'vitest'
import { canvasMetrics, clampPage, createRenderGate, fitToWidthScale } from './pdf'

describe('clampPage', () => {
  it('keeps a page inside the document', () => {
    expect(clampPage(3, 10)).toBe(3)
  })

  it('clamps past either end instead of going out of bounds', () => {
    expect(clampPage(0, 10)).toBe(1)
    expect(clampPage(-5, 10)).toBe(1)
    expect(clampPage(11, 10)).toBe(10)
  })

  it('floors fractional pages', () => {
    expect(clampPage(2.7, 10)).toBe(2)
  })

  // An empty or not-yet-loaded document must still yield a renderable page
  // number rather than 0, which pdf.js rejects.
  it('never returns less than one, even for an empty document', () => {
    expect(clampPage(1, 0)).toBe(1)
    expect(clampPage(5, 0)).toBe(1)
  })
})

describe('fitToWidthScale', () => {
  it('scales the page up to fill the available width', () => {
    expect(fitToWidthScale(1000, 500)).toBe(2)
  })

  it('scales the page down when it is wider than the viewport', () => {
    expect(fitToWidthScale(250, 500)).toBe(0.5)
  })

  // A viewport can measure 0 before layout settles; a 0 or negative scale
  // makes pdf.js throw, so fall back to 1:1.
  it('falls back to 1 when either dimension is unusable', () => {
    expect(fitToWidthScale(0, 500)).toBe(1)
    expect(fitToWidthScale(1000, 0)).toBe(1)
    expect(fitToWidthScale(-10, 500)).toBe(1)
    expect(fitToWidthScale(Number.NaN, 500)).toBe(1)
  })
})

describe('canvasMetrics', () => {
  it('sizes the backing store by the pixel ratio and the element in CSS pixels', () => {
    const metrics = canvasMetrics({ width: 400, height: 600 }, 2)
    expect(metrics).toEqual({
      cssWidth: 400,
      cssHeight: 600,
      pixelWidth: 800,
      pixelHeight: 1200,
    })
  })

  it('rounds the backing store to whole pixels', () => {
    const metrics = canvasMetrics({ width: 100.4, height: 200.6 }, 1.5)
    expect(metrics.pixelWidth).toBe(151)
    expect(metrics.pixelHeight).toBe(301)
  })

  it('treats a missing or absurd pixel ratio as 1', () => {
    expect(canvasMetrics({ width: 100, height: 100 }, 0).pixelWidth).toBe(100)
    expect(canvasMetrics({ width: 100, height: 100 }, Number.NaN).pixelWidth).toBe(100)
  })
})

describe('createRenderGate', () => {
  function fakeTask() {
    let settle: () => void = () => {}
    const promise = new Promise<void>((resolve) => {
      settle = resolve
    })
    return { cancel: vi.fn(), promise, settle }
  }

  // Two renders onto one canvas interleave their draws, so starting a render
  // must cancel whatever is still in flight.
  it('cancels the in-flight render when a new one starts', async () => {
    const gate = createRenderGate()
    const first = fakeTask()
    const second = fakeTask()

    const firstRun = gate.run(() => first)
    const secondRun = gate.run(() => second)

    expect(first.cancel).toHaveBeenCalledTimes(1)
    expect(second.cancel).not.toHaveBeenCalled()

    first.settle()
    second.settle()
    await Promise.all([firstRun, secondRun])
  })

  it('does not cancel a render that already finished', async () => {
    const gate = createRenderGate()
    const first = fakeTask()

    const firstRun = gate.run(() => first)
    first.settle()
    await firstRun

    const second = fakeTask()
    const secondRun = gate.run(() => second)
    expect(first.cancel).not.toHaveBeenCalled()

    second.settle()
    await secondRun
  })

  it('cancels the in-flight render on teardown', async () => {
    const gate = createRenderGate()
    const task = fakeTask()

    const run = gate.run(() => task)
    gate.cancel()

    expect(task.cancel).toHaveBeenCalledTimes(1)
    task.settle()
    await run
  })

  // pdf.js rejects a cancelled render with a RenderingCancelledException; that
  // is expected control flow, not an error the route should surface.
  it('swallows a cancellation rejection but surfaces a real failure', async () => {
    const gate = createRenderGate()
    const cancelled = {
      cancel: vi.fn(),
      promise: Promise.reject(new Error('Rendering cancelled, page 3')),
    }
    await expect(gate.run(() => cancelled)).resolves.toBeUndefined()

    const broken = {
      cancel: vi.fn(),
      promise: Promise.reject(new Error('bad XRef entry')),
    }
    await expect(gate.run(() => broken)).rejects.toThrow('bad XRef entry')
  })
})
