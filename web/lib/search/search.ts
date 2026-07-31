import type { FilterState, SearchHit } from './types'

function matchesQuery(hit: SearchHit, q: string): boolean {
  if (q === '') return true
  const needle = q.toLowerCase()
  const haystack = [
    hit.title,
    hit.edition_title ?? '',
    hit.work_title ?? '',
    hit.authors.join(' '),
    hit.snippet_text ?? '',
  ]
    .join('\n')
    .toLowerCase()
  return haystack.includes(needle)
}

function matchesFormat(hit: SearchHit, formats: Set<string>): boolean {
  if (formats.size === 0) return true
  return hit.format !== null && formats.has(hit.format)
}

function matchesLanguage(hit: SearchHit, languages: Set<string>): boolean {
  if (languages.size === 0) return true
  return hit.language !== null && languages.has(hit.language)
}

export function filterHits(
  query: string,
  filters: FilterState,
  hits: readonly SearchHit[],
): SearchHit[] {
  return hits.filter(
    (h) =>
      matchesQuery(h, query.trim()) &&
      matchesFormat(h, filters.formats) &&
      matchesLanguage(h, filters.languages),
  )
}
