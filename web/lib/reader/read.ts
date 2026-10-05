import { invoke } from '@tauri-apps/api/core'

export interface ReaderPublicationPayload {
  baseUrl: string
  manifest: unknown
  positions: unknown
}

interface EpubPublicationIpc {
  kind: 'Epub'
  edition_id: string
  title: string | null
  base_url: string
  manifest: string
  positions: string
}

interface PdfPublicationIpc {
  kind: 'Pdf'
  edition_id: string
  title: string | null
  pdf_url: string
}

type ReaderPublicationIpc = EpubPublicationIpc | PdfPublicationIpc | { kind: string } | null

function isEpubPublication(payload: ReaderPublicationIpc): payload is EpubPublicationIpc {
  return payload !== null && payload.kind === 'Epub'
}

function isPdfPublication(payload: ReaderPublicationIpc): payload is PdfPublicationIpc {
  return payload !== null && payload.kind === 'Pdf'
}

export async function openReaderWindow(editionId: string): Promise<void> {
  console.debug('[reader] requesting open_reader', { editionId })
  try {
    await invoke('open_reader', { editionId })
    console.debug('[reader] open_reader completed', { editionId })
  } catch (error) {
    console.error('[reader] open_reader failed', { editionId, error })
    throw error
  }
}

function parseJsonField(raw: string, field: string, editionId: string): unknown {
  try {
    return JSON.parse(raw)
  } catch {
    throw new Error(
      `Could not parse the ${field} for edition ${editionId}. Try re-importing the file.`,
    )
  }
}

export async function loadEpubReaderPublication(
  editionId: string,
): Promise<ReaderPublicationPayload> {
  const payload = await invoke<ReaderPublicationIpc>('reader_publication', {
    editionId,
  })
  // The backend serves a tagged union: audiobooks resolve to the `Audiobook`
  // variant and missing editions to `null`. The EPUB navigator only accepts
  // the `Epub` variant.
  if (payload === null) {
    throw new Error(`No publication found for edition ${editionId}.`)
  }
  if (!isEpubPublication(payload)) {
    throw new Error(`Edition ${editionId} is not an EPUB publication.`)
  }
  const manifest = parseJsonField(payload.manifest, 'publication manifest', editionId)
  if (
    typeof manifest !== 'object' ||
    manifest === null ||
    !Array.isArray((manifest as { readingOrder?: unknown }).readingOrder) ||
    (manifest as { readingOrder: unknown[] }).readingOrder.length === 0
  ) {
    throw new Error(
      `The publication manifest for edition ${editionId} has no reading order. Try re-importing the file.`,
    )
  }
  const positions = parseJsonField(payload.positions, 'reading positions', editionId)
  if (!Array.isArray(positions)) {
    throw new Error(
      `The reading positions for edition ${editionId} are malformed. Try re-importing the file.`,
    )
  }
  return { baseUrl: payload.base_url, manifest, positions }
}

export async function readReaderResource(editionId: string, href: string): Promise<string> {
  return invoke<string>('reader_resource', { editionId, href })
}

export interface PdfReaderPublication {
  title: string | null
  /** Loopback URL pdf.js fetches, carrying the per-launch bearer token. */
  pdfUrl: string
}

/**
 * Fetch the PDF descriptor for an edition (ADR 0037).
 *
 * The backend serves one tagged union for every reader, so a non-PDF variant
 * reaching here means the window was opened for the wrong edition.
 */
export async function loadPdfReaderPublication(editionId: string): Promise<PdfReaderPublication> {
  const payload = await invoke<ReaderPublicationIpc>('reader_publication', { editionId })
  if (payload === null) {
    throw new Error(`No publication found for edition ${editionId}.`)
  }
  if (!isPdfPublication(payload)) {
    throw new Error(`Edition ${editionId} is not a PDF publication.`)
  }
  return { title: payload.title, pdfUrl: payload.pdf_url }
}

export interface ReadableFile {
  file_path?: string | null
  file_format?: string | null
  file_status?: string | null
}

/** Formats with a reader window: EPUB via Readium, PDF via pdf.js (ADR 0037). */
const READABLE_FORMATS = ['epub', 'pdf']

export function canOpenInReader(file: ReadableFile | null | undefined): boolean {
  if (!file || file.file_status === 'missing') return false
  if (file.file_format) {
    return READABLE_FORMATS.includes(file.file_format.toLowerCase())
  }
  const path = file.file_path?.toLowerCase()
  if (!path) return false
  return READABLE_FORMATS.some((format) => path.endsWith(`.${format}`))
}
