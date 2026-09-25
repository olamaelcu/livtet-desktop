import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  commands,
  type EditionFile,
  type ImportBatchResult,
  type ImportError,
  type ImportFileResult,
  type ImportMode,
  type ImportOutcome,
} from '../bindings'

export type {
  EditionFile,
  ImportBatchResult,
  ImportError,
  ImportFileResult,
  ImportMode,
  ImportOutcome,
}

const BATCH_EVENT = 'import://batch'
const FILE_EVENT = 'import://file'

export type ImportPhase = 'started' | 'finished'

export interface ImportBatchProgress {
  phase: ImportPhase
  total: number
  result?: ImportBatchResult
}

export interface ImportFileProgress {
  index: number
  total: number
  path: string
  phase: ImportPhase
  outcome?: ImportOutcome
  error?: ImportError
}

export type ImportRowStatus = 'pending' | 'importing' | 'imported' | 'duplicate' | 'failed'

export interface ImportRow {
  path: string
  status: ImportRowStatus
  message?: string
}

const IMPORT_ERROR_MESSAGES: Record<string, string> = {
  parse: 'Could not read metadata from this file.',
  database: 'The library database rejected this file.',
  index: 'The search index could not be updated.',
  io: 'The file could not be read.',
  importer: 'The importer failed on this file.',
  unsupported: 'No importer supports this file type.',
}

/** Human copy for a failed import, falling back to the backend message. */
export function importErrorMessage(error: ImportError): string {
  return IMPORT_ERROR_MESSAGES[error.code] ?? error.message
}

/** Import one file, throwing the backend error on failure. */
export async function importFile(path: string, mode: ImportMode = 'link'): Promise<ImportOutcome> {
  const result = await commands.importFile(path, mode)
  if (result.status === 'error') throw result.error
  return result.data
}

/**
 * Import a batch. Absent or empty input is a no-op that resolves to an empty
 * batch without invoking IPC, so callers can pass picker output directly.
 */
export async function importFiles(
  paths: readonly string[] | null | undefined,
  mode: ImportMode = 'link',
): Promise<ImportBatchResult> {
  const list = (paths ?? []).filter((path) => path.length > 0)
  if (list.length === 0) return { files: [], imported: 0, duplicated: 0, failed: 0 }
  return commands.importFiles(list, mode)
}

/** Re-link a missing library file at a newly picked source. */
export async function relinkEditionFile(editionId: string, path: string): Promise<EditionFile> {
  const result = await commands.relinkEditionFile(editionId, path)
  if (result.status === 'error') throw result.error
  return result.data
}

function statusFromFile(file: ImportFileResult): Pick<ImportRow, 'status' | 'message'> {
  if (file.error) return { status: 'failed', message: importErrorMessage(file.error) }
  if (file.outcome?.duplicate) return { status: 'duplicate' }
  return { status: 'imported' }
}

/** Fold a live file event into the row list, matching by path. */
export function reduceImportEvents(rows: ImportRow[], event: ImportFileProgress): ImportRow[] {
  if (event.phase === 'started') {
    return rows.map((row) =>
      row.path === event.path ? { ...row, status: 'importing', message: undefined } : row,
    )
  }
  if (event.outcome || event.error) {
    const next = statusFromFile({
      path: event.path,
      outcome: event.outcome ?? null,
      error: event.error ?? null,
    })
    return rows.map((row) => (row.path === event.path ? { ...row, ...next } : row))
  }
  return rows
}

/** Overlay the authoritative batch result onto the row list. */
export function mergeBatchResult(rows: ImportRow[], result: ImportBatchResult): ImportRow[] {
  const byPath = new Map(result.files.map((file) => [file.path, file]))
  return rows.map((row) => {
    const file = byPath.get(row.path)
    return file ? { ...row, ...statusFromFile(file) } : row
  })
}

/** Subscribe to import progress. Resolves to a function that removes listeners. */
export async function listenToImportEvents(handlers: {
  batch?: (payload: ImportBatchProgress) => void
  file?: (payload: ImportFileProgress) => void
}): Promise<UnlistenFn> {
  const unlisteners: UnlistenFn[] = []
  try {
    if (handlers.batch) {
      unlisteners.push(
        await listen<ImportBatchProgress>(BATCH_EVENT, (event) => handlers.batch?.(event.payload)),
      )
    }
    if (handlers.file) {
      unlisteners.push(
        await listen<ImportFileProgress>(FILE_EVENT, (event) => handlers.file?.(event.payload)),
      )
    }
  } catch (error) {
    for (const unlisten of unlisteners) unlisten()
    throw error
  }
  return () => {
    for (const unlisten of unlisteners) unlisten()
  }
}
