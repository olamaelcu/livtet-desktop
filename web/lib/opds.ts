import { invoke } from '@tauri-apps/api/core'
import type {
  ImportOutcome,
  OpdsAuthInput,
  OpdsCatalog,
  OpdsFeed,
  OpdsNavigation,
  OpdsPreset,
  OpdsPublication,
} from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type {
  ImportOutcome,
  OpdsAuthInput,
  OpdsCatalog,
  OpdsFeed,
  OpdsNavigation,
  OpdsPreset,
  OpdsPublication,
}

export async function loadCatalogs(): Promise<OpdsCatalog[]> {
  return invoke<OpdsCatalog[]>('opds_catalogs_list')
}

export async function loadPresets(): Promise<OpdsPreset[]> {
  return invoke<OpdsPreset[]>('opds_default_catalogs')
}

export async function createCatalog(input: {
  title: string
  feedUrl: string
  auth: OpdsAuthInput
}): Promise<OpdsCatalog> {
  return invoke<OpdsCatalog>('opds_catalogs_create', input)
}

export async function updateCatalog(input: {
  id: string
  title?: string
  feedUrl?: string
  auth?: OpdsAuthInput
}): Promise<OpdsCatalog> {
  return invoke<OpdsCatalog>('opds_catalogs_update', {
    id: input.id,
    title: input.title ?? null,
    feedUrl: input.feedUrl ?? null,
    auth: input.auth ?? null,
  })
}

export async function removeCatalog(id: string): Promise<void> {
  return invoke<void>('opds_catalogs_remove', { id })
}

export async function testCatalog(id: string): Promise<OpdsFeed> {
  return invoke<OpdsFeed>('opds_catalogs_test', { id })
}

export async function loadFeed(catalogId: string): Promise<OpdsFeed> {
  return invoke<OpdsFeed>('opds_feed', { id: catalogId })
}

export async function loadPage(catalogId: string, href: string): Promise<OpdsFeed> {
  return invoke<OpdsFeed>('opds_page', { id: catalogId, href })
}

export async function searchCatalog(catalogId: string, query: string): Promise<OpdsFeed> {
  return invoke<OpdsFeed>('opds_search', { id: catalogId, query })
}

export async function acquireItem(catalogId: string, itemUrl: string): Promise<ImportOutcome> {
  return invoke<ImportOutcome>('opds_acquire', { id: catalogId, itemUrl })
}

/** Normalize an OPDS error (serde-tagged) or string into a display message. */
export function opdsErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object') {
    const value = Object.values(error as Record<string, unknown>)[0]
    if (typeof value === 'string') return value
    if (value && typeof value === 'object' && 'message' in value) {
      return String((value as { message: unknown }).message)
    }
  }
  return 'OPDS request failed'
}
