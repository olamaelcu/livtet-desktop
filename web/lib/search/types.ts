// Re-export the specta-generated SearchHitRow as the local
// SearchHit type so the rest of the search module can keep
// referencing a stable name. The specta binding is the source
// of truth.
export type { SearchHitRow as SearchHit } from "$lib/bindings";

export interface FilterState {
  formats: Set<string>;
  languages: Set<string>;
}

export function emptyFilters(): FilterState {
  return { formats: new Set(), languages: new Set() };
}
