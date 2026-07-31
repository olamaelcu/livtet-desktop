import type { HitKind, SearchHit } from '$lib/bindings'

export function hit(overrides: Partial<SearchHit> = {}): SearchHit {
  return {
    kind: 'work' as HitKind,
    edition_id: null,
    work_id: '/work/test-1',
    author_id: null,
    title: 'Untitled',
    work_title: null,
    edition_title: null,
    authors: [],
    isbn: null,
    format: null,
    language: null,
    published_date: null,
    score: null,
    explanation: null,
    snippet_text: null,
    snippet_highlighted: [],
    grouped_edition_ids: [],
    source: 'local',
    publisher: null,
    page_count: null,
    cover_url: null,
    description: null,
    isbn_13: null,
    blurhash: null,
    dominant_color: null,
    in_catalog: false,
    in_catalog_edition_id: null,
    ...overrides,
  }
}
