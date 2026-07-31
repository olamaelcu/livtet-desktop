import type { DigitalInventoryRow } from "$lib/bindings";
import type { SearchHit } from "$lib/search/types";

/**
 * Best-effort resolver for which edition a search hit should open
 * when the user clicks it. An edition can carry multiple files (e.g.
 * epub + audio), so when `files` is present we trust its
 * `edition_id`. Otherwise we fall back to `hit.edition_id`, which is
 * null for `work`/`person` hits.
 */
export function editionForHit(
  hit: SearchHit,
  files?: DigitalInventoryRow | null,
): { editionId: string | null } {
  if (files) return { editionId: files.edition_id };
  return { editionId: hit.edition_id };
}