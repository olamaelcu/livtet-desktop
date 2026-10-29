import { commands, type ImportOutcome, type ShelvedBook } from './bindings'

export type { ShelvedBook }

/**
 * Shelf keys in the order the library shows them, matching the local reading
 * statuses (`livtet_core::shelf::ReadingStatus`).
 */
export const SHELF_ORDER = ['reading', 'read', 'to-read', 'dnf'] as const

/** Every book the signed-in account shelves, across all of its shelves. */
export async function listShelvedBooks(): Promise<ShelvedBook[]> {
  const result = await commands.listShelvedBooks()
  if (result.status === 'error') throw new Error(result.error.message)
  return result.data
}

/**
 * Import one shelved book as a virtual book. The shelf it sits on becomes the
 * local reading status; nothing is published back to the account.
 */
export async function importShelvedBook(book: ShelvedBook): Promise<ImportOutcome> {
  const result = await commands.importShelvedBook(book)
  if (result.status === 'error') throw new Error(result.error.message)
  return result.data
}
