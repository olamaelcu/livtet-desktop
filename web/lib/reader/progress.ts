import { commands } from '$lib/bindings'
import type { HighlightColor } from './highlights'

/** One user annotation tied to a location in a book. */
export interface Annotation {
  /** Client-generated stable id (`crypto.randomUUID()`). */
  id: string
  /** The note text the reader typed. */
  text: string
  /** Text selected in the publication when the note was written, if any. */
  quote?: string
  /** Serialized Readium locator, for EPUB editions. */
  locator?: string
  /** 1-based page number, for PDF editions. */
  pageNumber?: number
  /** Highlight colour key; absent means the palette default. */
  color?: HighlightColor
  /** ISO timestamp of when the note was written; absent on notes that predate it. */
  createdAt?: string
}

export interface ReaderProgress {
  editionId: string
  kind: 'epub' | 'pdf' | 'audio'
  epub?: { locator: string } // JSON-serialized Locator
  pdf?: { pageNumber: number }
  audio?: { positionSeconds: number }
  annotations?: Annotation[]
}

/** Date a note was written, in the reader's locale. */
const noteDate = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

/**
 * Human-readable date for a note, or `undefined` when it has none.
 *
 * Notes written before `createdAt` was stored, and any timestamp the platform
 * cannot parse, render as absent rather than "Invalid Date".
 */
export function formatNoteDate(createdAt: string | undefined): string | undefined {
  if (!createdAt) return undefined
  const parsed = new Date(createdAt)
  if (Number.isNaN(parsed.getTime())) return undefined
  return noteDate.format(parsed)
}

/** Decode the `annotations` JSON column, tolerating absent or malformed data. */
function parseAnnotations(raw: string | null | undefined): Annotation[] | undefined {
  if (!raw) return undefined
  try {
    const parsed: unknown = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as Annotation[]) : undefined
  } catch {
    return undefined
  }
}

/**
 * Resolve an edition's last position from the SQLite `reading_progress` table.
 * Returns `null` when nothing has been stored (or the backend is unreachable).
 */
export async function loadProgress(editionId: string): Promise<ReaderProgress | null> {
  try {
    const result = await commands.getReaderPosition(editionId)
    if (result.status !== 'ok' || !result.data) return null
    const row = result.data
    const annotations = parseAnnotations(row.annotations)
    if (row.kind === 'epub' && row.locator) {
      return { editionId, kind: 'epub', epub: { locator: row.locator }, annotations }
    }
    if (row.kind === 'pdf' && row.page_number != null) {
      return { editionId, kind: 'pdf', pdf: { pageNumber: row.page_number }, annotations }
    }
    if (row.kind === 'audio' && row.position_seconds != null) {
      return {
        editionId,
        kind: 'audio',
        audio: { positionSeconds: row.position_seconds },
        annotations,
      }
    }
    return null
  } catch {
    return null
  }
}

/**
 * Stored annotations for an edition.
 *
 * Read directly rather than through {@link loadProgress}, which returns `null`
 * when the row's reader kind does not match — a book with notes but no stored
 * position would otherwise appear note-less.
 */
export async function loadAnnotations(editionId: string): Promise<Annotation[]> {
  try {
    const result = await commands.getReaderPosition(editionId)
    if (result.status !== 'ok' || !result.data) return []
    return parseAnnotations(result.data.annotations) ?? []
  } catch {
    return []
  }
}

/**
 * Persist an edition's position to the SQLite `reading_progress` table.
 *
 * Failures are swallowed: a transient IPC/database error must not break the
 * reader, and the next position change will retry the write.
 */
export async function saveProgress(
  editionId: string,
  progress: Omit<ReaderProgress, 'editionId'>,
): Promise<boolean> {
  try {
    const result = await commands.saveReaderPosition(
      editionId,
      progress.kind,
      progress.epub?.locator ?? null,
      progress.pdf?.pageNumber ?? null,
      progress.audio?.positionSeconds ?? null,
      progress.annotations ? JSON.stringify(progress.annotations) : null,
    )
    return result.status === 'ok'
  } catch {
    // backend unavailable — the reader keeps running without persisting
    return false
  }
}

// Debounced save for rapid position changes (250ms default)
export function debouncedSave(
  currentEditionId: () => string,
  build: () => Omit<ReaderProgress, 'editionId'>,
  ms = 250,
): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined
  return () => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      timer = undefined
      void saveProgress(currentEditionId(), build())
    }, ms)
  }
}
