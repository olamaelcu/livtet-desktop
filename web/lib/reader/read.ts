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

type ReaderPublicationIpc = EpubPublicationIpc | { kind: string } | null

function isEpubPublication(payload: ReaderPublicationIpc): payload is EpubPublicationIpc {
  return payload !== null && payload.kind === 'Epub'
}

export async function openReader(editionId: string): Promise<void> {
  await invoke('open_reader', { edition_id: editionId })
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

export async function loadReaderPublication(editionId: string): Promise<ReaderPublicationPayload> {
  const payload = await invoke<ReaderPublicationIpc>('reader_publication', {
    edition_id: editionId,
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
  return invoke<string>('reader_resource', { edition_id: editionId, href })
}

export interface ReadableFile {
  file_path?: string | null
  file_format?: string | null
  file_status?: string | null
}

export function canOpenInReader(file: ReadableFile | null | undefined): boolean {
  if (!file || file.file_status === 'missing') return false
  if (file.file_format) return file.file_format.toLowerCase() === 'epub'
  return file.file_path?.toLowerCase().endsWith('.epub') ?? false
}
