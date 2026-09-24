import { invoke } from '@tauri-apps/api/core'
import type { BulkError, DeleteOutcome, ExportOutcome, TagMutationOutcome } from '../bindings'
import type { EditionFilters } from '../search'

export type { BulkError, DeleteOutcome, ExportOutcome, TagMutationOutcome }

export async function deleteEditions(editionIds: readonly string[]): Promise<DeleteOutcome> {
  if (editionIds.length === 0) {
    return { deleted: 0, files_removed: 0, covers_removed: 0, skipped: [] }
  }
  return invoke<DeleteOutcome>('delete_editions', { editionIds })
}

export async function exportEditionsCsv(
  editionIds: readonly string[],
  path: string,
): Promise<ExportOutcome> {
  return invoke<ExportOutcome>('export_editions_csv', { editionIds, path })
}

export async function addEditionTags(
  editionIds: readonly string[],
  tag: string,
): Promise<TagMutationOutcome> {
  return invoke<TagMutationOutcome>('add_edition_tags', { editionIds, tag })
}

export async function removeEditionTags(
  editionIds: readonly string[],
  tagId: string,
): Promise<TagMutationOutcome> {
  return invoke<TagMutationOutcome>('remove_edition_tags', { editionIds, tagId })
}

export async function matchingEditionIds(
  query?: string,
  filters?: EditionFilters,
): Promise<string[]> {
  return invoke<string[]>('matching_edition_ids', {
    query: query ?? null,
    filters: filters ?? null,
  })
}
