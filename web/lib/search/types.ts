// TypeScript mirror of livtet_search::SearchHit from
// core/livtet-search/src/lib.rs. Field names are snake_case to match
// the JSON emitted by specta. Update this file when the Rust struct
// changes — do not introduce field aliases or renames here.

export type HitKind = "edition" | "work" | "person";

export interface SearchHit {
  kind: HitKind;
  edition_id: string | null;
  work_id: string;
  author_id: string | null;
  title: string;
  work_title: string | null;
  edition_title: string | null;
  authors: string[];
  isbn: string | null;
  format: string | null;
  language: string | null;
  published_date: string | null;
  score: number;
  explanation: string | null;
  snippet_text: string | null;
  /** Rust `Range<usize>` serialises as `[start, end]`. */
  snippet_highlighted: [number, number][];
  grouped_edition_ids: string[];
  source: string;
}

export interface FilterState {
  formats: Set<string>;
  languages: Set<string>;
}

export function emptyFilters(): FilterState {
  return { formats: new Set(), languages: new Set() };
}
