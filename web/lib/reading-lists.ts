import { invoke } from '@tauri-apps/api/core'
import type { ReadingList, ReadingListError, ReadingListPatch } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { ReadingList, ReadingListError, ReadingListPatch }

/** Every reading list, each with its edition ids in position order. */
export async function listReadingLists(): Promise<ReadingList[]> {
  return invoke<ReadingList[]>('list_reading_lists')
}

/** One reading list by id, with its edition ids in position order. */
export async function getReadingList(listId: string): Promise<ReadingList> {
  return invoke<ReadingList>('get_reading_list', { listId })
}

/**
 * Rename and/or re-describe a list. Only the supplied fields change; an empty
 * description clears it, and a blank name is rejected by the backend.
 */
export async function updateReadingList(
  listId: string,
  patch: ReadingListPatch,
): Promise<ReadingList> {
  return invoke<ReadingList>('update_reading_list', { listId, patch })
}

/**
 * Replace the whole book order. `editionIds` must be a permutation of the
 * list's current members; a duplicate, an outsider, or an omission is rejected
 * with the engine's message.
 */
export async function reorderReadingList(
  listId: string,
  editionIds: string[],
): Promise<ReadingList> {
  return invoke<ReadingList>('reorder_reading_list', { listId, editionIds })
}

/**
 * Unwraps a `ReadingListError` (`{ code, message }`) or a plain string into a
 * human-readable message. Mirrors `mcpErrorMessage`.
 */
export function readingListErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error
    if (typeof message === 'string' && message) return message
  }
  return 'Reading list request failed'
}
