import {
  commands,
  type ListeningProgress,
  type ReaderChapter,
  type ReaderPublication,
} from './bindings'

// Strict re-export of generated contract — single source of truth
export type { ListeningProgress, ReaderChapter, ReaderPublication }

/** Unwrap a command envelope, throwing the backend error on failure. */
function unwrap<T>(result: { status: 'ok' | 'error'; data?: T; error?: unknown }): T {
  if (result.status === 'error') throw result.error
  return result.data as T
}

export async function loadReaderPublication(editionId: string): Promise<ReaderPublication | null> {
  return unwrap(await commands.readerPublication(editionId))
}

export async function openReader(editionId: string): Promise<null> {
  return unwrap(await commands.openReader(editionId))
}

export async function loadListeningProgress(editionId: string): Promise<ListeningProgress | null> {
  return unwrap(await commands.getListeningProgress(editionId))
}

export async function saveListeningProgress(
  editionId: string,
  positionSeconds: number,
): Promise<ListeningProgress> {
  return unwrap(await commands.saveListeningProgress(editionId, positionSeconds))
}

/** Format seconds as `H:MM:SS`; non-finite input renders as zero. */
export function formatTimestamp(totalSeconds: number): string {
  const clamped = Number.isFinite(totalSeconds) ? Math.max(0, Math.floor(totalSeconds)) : 0
  const hours = Math.floor(clamped / 3600)
  const minutes = Math.floor((clamped % 3600) / 60)
  const seconds = clamped % 60
  return `${hours}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`
}

/** Index of the chapter containing `positionSeconds`, or -1 when empty. */
export function chapterAt(chapters: ReaderChapter[], positionSeconds: number): number {
  let current = -1
  for (let index = 0; index < chapters.length; index += 1) {
    if (positionSeconds >= chapters[index].audio_start) current = index
    else break
  }
  return current
}
