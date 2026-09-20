import { invoke } from '@tauri-apps/api/core';
import type { SearchResult as BindingResult, SearchResponse as BindingResponse, HitKind } from './bindings';

export type { HitKind };
export interface Author {
  name: string;
  role: 'author' | 'editor' | 'translator';
}

export interface Edition {
  id: string;
  title: string;
  authors: Author[];
  published: string;
  description: string;
  cover_url?: string;
}

// Strict re-export of generated contract — single source of truth
export type SearchResult = BindingResult;
export type SearchResponse = BindingResponse;

const LIMIT = 20;

export async function loadEditions(
  query?: string,
  offset?: number,
  limit?: number
): Promise<SearchResponse> {
  return invoke<SearchResponse>('search_editions', {
    query: query ?? null,
    offset: offset ?? 0,
    limit: limit ?? LIMIT
  });
}

export async function searchTypeahead(query: string, limit = 10): Promise<SearchResult[]> {
  if (!query.trim()) return [];
  return invoke<SearchResult[]>('search_typeahead', { query, limit });
}

export async function countEditions(query?: string): Promise<number> {
  return invoke<number>('search_editions_count', { query: query ?? null });
}

export function mapHitToEdition(hit: SearchResult): Edition {
  return {
    id: hit.edition_id ?? hit.work_id,
    title: hit.title,
    authors: (hit.authors ?? []).map((name: string, idx: number) => ({
      name,
      role: (idx === 0 ? 'author' : 'editor') as Author['role']
    })),
    published: hit.pub_date ?? '',
    description: hit.snippet_text ?? '',
    cover_url: undefined
  };
}
