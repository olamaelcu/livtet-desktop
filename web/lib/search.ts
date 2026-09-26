import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import type {
  SearchResponse as BindingResponse,
  SearchResult as BindingResult,
  EditionCover,
  EditionDetail,
  EditionFilters,
  FilterOption,
  FilterOptions,
  HitKind,
} from './bindings'

// Strict re-export of generated contract — single source of truth
export type { EditionCover, EditionDetail, EditionFilters, FilterOption, FilterOptions, HitKind }
export interface Author {
  name: string
  role: 'author' | 'editor' | 'translator'
}

export interface Edition {
  id: string
  edition_id: string | null
  kind: HitKind
  title: string
  authors: Author[]
  published: string
  description: string
  /** Whether the edition has a file on disk (`digital_inventory` row). */
  has_file: boolean
}

// Strict re-export of generated contract — single source of truth
export type SearchResult = BindingResult
export type SearchResponse = BindingResponse

const LIMIT = 20

export async function loadEditions(
  query?: string,
  filters?: EditionFilters,
  offset?: number,
  limit?: number,
): Promise<SearchResponse> {
  return invoke<SearchResponse>('search_editions', {
    query: query ?? null,
    filters: filters ?? null,
    offset: offset ?? 0,
    limit: limit ?? LIMIT,
  })
}

export async function searchTypeahead(query: string, limit = 10): Promise<SearchResult[]> {
  if (!query.trim()) return []
  return invoke<SearchResult[]>('search_typeahead', { query, limit })
}

export async function loadFilterOptions(): Promise<FilterOptions> {
  return invoke<FilterOptions>('filter_options')
}

export function mapHitToEdition(hit: SearchResult): Edition {
  return {
    id: hit.edition_id ?? hit.work_id,
    edition_id: hit.edition_id,
    kind: hit.kind,
    title: hit.title,
    authors: (hit.authors ?? []).map((name: string, idx: number) => ({
      name,
      role: (idx === 0 ? 'author' : 'editor') as Author['role'],
    })),
    published: hit.pub_date ?? '',
    description: hit.snippet_text ?? '',
    has_file: hit.has_file,
  }
}

/** Fetch one edition's full catalog record for the detail drawer. */
export async function loadEditionDetail(editionId: string): Promise<EditionDetail | null> {
  return invoke<EditionDetail | null>('get_edition_detail', { editionId })
}

/** Resolve the cover paths for a batch of editions (no cover → omitted). */
export async function loadEditionCovers(editionIds: string[]): Promise<EditionCover[]> {
  if (editionIds.length === 0) return []
  return invoke<EditionCover[]>('get_edition_covers', { editionIds })
}

/** Resolve an on-disk cover path to an `asset:` URL the webview can load. */
export function coverUrlFor(path: string | null | undefined): string | undefined {
  return path ? convertFileSrc(path) : undefined
}
