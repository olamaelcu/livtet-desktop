import '@awesome.me/webawesome/dist/components/button/button.js'
import '@awesome.me/webawesome/dist/components/icon/icon.js'
import { expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

const { closeWindow } = vi.hoisted(() => ({ closeWindow: vi.fn() }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ close: closeWindow }),
}))

// The worker is a bundled asset URL in the real build; a browser test needs
// only that the import resolves.
vi.mock('pdfjs-dist/build/pdf.worker.min.mjs?url', () => ({
  default: '/pdfjs/pdf.worker.min.mjs',
}))

const { getDocument, globalWorkerOptions, renderTask } = vi.hoisted(() => ({
  getDocument: vi.fn(),
  globalWorkerOptions: { workerSrc: '' },
  renderTask: { cancel: vi.fn(), promise: Promise.resolve() },
}))

vi.mock('pdfjs-dist', () => ({
  GlobalWorkerOptions: globalWorkerOptions,
  getDocument,
}))

import pdfPage from '../../routes/reader/pdf/[editionId]/+page.svelte?raw'
import PdfReaderFixture from './fixtures/PdfReaderFixture.svelte'

/** A pdf.js document stand-in: two pages, each 500x800 unscaled. */
function fakeDocument(pageCount = 2) {
  return {
    numPages: pageCount,
    getPage: vi.fn(async () => ({
      getViewport: ({ scale }: { scale: number }) => ({
        width: 500 * scale,
        height: 800 * scale,
      }),
      render: vi.fn(() => renderTask),
    })),
  }
}

function stubPublication() {
  invoke.mockResolvedValue({
    kind: 'Pdf',
    edition_id: '01hzzzzzzzzzzzzzzzzzzzzzzz',
    title: 'A Scanned Book',
    pdf_url: 'http://127.0.0.1:9000/pdf/01hzzzzzzzzzzzzzzzzzzzzzzz?t=tok',
  })
  getDocument.mockReturnValue({
    promise: Promise.resolve(fakeDocument()),
    destroy: vi.fn(async () => {}),
  })
}

test('the PDF route renders exactly one scroll region inside the reader layout', async () => {
  stubPublication()
  const { container } = await render(PdfReaderFixture)

  const frame = container.firstElementChild as HTMLElement
  expect(getComputedStyle(frame).overflowY).toBe('hidden')
  expect(container.querySelectorAll('[data-scroll-region]')).toHaveLength(1)

  const region = container.querySelector<HTMLElement>('[data-scroll-region]') as HTMLElement
  expect(getComputedStyle(region).overflowY).toBe('auto')
  // Bounded by the frame rather than stretching it, like the other readers.
  expect(region.clientHeight).toBeGreaterThan(0)
  expect(region.clientHeight).toBeLessThanOrEqual(frame.clientHeight)
})

// Guards against the layout assertions above passing vacuously: the viewport
// renders in every phase, so prove the document actually loaded and drew.
test('the PDF route reaches the ready phase and reports the page count', async () => {
  stubPublication()
  const { container } = await render(PdfReaderFixture)

  await vi.waitFor(() => {
    expect(container.textContent).toContain('1 / 2')
  })
  expect(container.textContent).toContain('A Scanned Book')
  expect(container.querySelector('.error')).toBeNull()
  expect(getDocument).toHaveBeenCalledWith(
    expect.objectContaining({
      url: 'http://127.0.0.1:9000/pdf/01hzzzzzzzzzzzzzzzzzzzzzzz?t=tok',
      // Without this the ranged loopback route buys nothing.
      disableAutoFetch: true,
      cMapUrl: '/pdfjs/cmaps/',
      standardFontDataUrl: '/pdfjs/standard_fonts/',
    }),
  )
})

test('the PDF route carries no navigation chrome', async () => {
  stubPublication()
  const { container } = await render(PdfReaderFixture)
  expect(container.querySelector('nav')).toBeNull()
  expect(container.querySelector('wa-button[href]')).toBeNull()
})

test('the PDF route has no height: 100% chain', () => {
  expect(pdfPage).not.toMatch(/height:\s*100%/)
  expect(pdfPage).toContain('<ReaderLayout>')
})
