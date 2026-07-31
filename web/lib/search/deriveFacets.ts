import type { SearchHit } from './types'

export interface Facets {
  formats: string[]
  languages: string[]
}

function uniqueSorted(values: Iterable<string | null>): string[] {
  const set = new Set<string>()
  for (const v of values) {
    if (v !== null) set.add(v)
  }
  return [...set].sort((a, b) => a.localeCompare(b))
}

export function deriveFacets(hits: readonly SearchHit[]): Facets {
  return {
    formats: uniqueSorted(hits.map((h) => h.format)),
    languages: uniqueSorted(hits.map((h) => h.language)),
  }
}
