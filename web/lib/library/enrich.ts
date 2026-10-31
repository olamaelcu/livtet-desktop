import {
  type CoverOffer,
  commands,
  type EnrichMode,
  type EnrichmentPreview,
  type EnrichmentSummary,
  type FieldAddition,
  type FieldChange,
  type FieldConflict,
  type FoundIdentifier,
  type PluginError,
} from '../bindings'

// Strict re-export of the generated contract — single source of truth.
export type {
  CoverOffer,
  EnrichMode,
  EnrichmentPreview,
  EnrichmentSummary,
  FieldAddition,
  FieldChange,
  FieldConflict,
  FoundIdentifier,
  PluginError,
}

/**
 * The identifier shape the drawer passes in, which is the catalog's
 * `IdentifierRef` widened to tolerate a record still loading.
 */
export interface EnrichIdentifier {
  kind?: string
  value?: string
}

/**
 * Fetch what the installed enrichers know about one edition, throwing the
 * `PluginError` on failure (ADR-0046). Nothing is written.
 */
export async function previewEnrichment(editionId: string): Promise<EnrichmentPreview> {
  const result = await commands.previewEnrichment(editionId)
  if (result.status === 'error') throw result.error
  return result.data
}

/**
 * Write the cached preview under `mode`, throwing the `PluginError` on failure.
 * The cache entry is consumed, so a second apply needs a fresh preview.
 */
export async function applyEnrichment(
  editionId: string,
  mode: EnrichMode,
): Promise<EnrichmentSummary> {
  const result = await commands.applyEnrichment(editionId, mode)
  if (result.status === 'error') throw result.error
  return result.data
}

/**
 * Whether the preview offers nothing at all. Sources that answered but found
 * nothing produce this, and it must not switch the drawer to a review screen.
 */
export function previewIsEmpty(preview: EnrichmentPreview): boolean {
  return (
    preview.added.length === 0 &&
    preview.unioned.length === 0 &&
    preview.conflicts.length === 0 &&
    preview.found.length === 0 &&
    preview.cover === null
  )
}

/**
 * Why enrichment cannot run for this edition, or `null` when it can.
 *
 * Enrichers are matched by identifier, so an edition without one has nothing to
 * look up: `preview_enrichment` would reject with `Invalid`. Saying so up front
 * beats a button that fails on click.
 */
export function enrichDisabledReason(
  identifiers: readonly EnrichIdentifier[] | undefined,
): string | null {
  const usable = (identifiers ?? []).filter((identifier) => (identifier.value ?? '').trim() !== '')
  if (usable.length === 0) {
    return 'Add an identifier (such as an ISBN) to this edition before enriching it.'
  }
  return null
}

function counted(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`
}

/**
 * One sentence describing what apply wrote, for the closing toast. The counts
 * are the ones apply reported, not the previewed ones, so `fill` and `overwrite`
 * tell different stories from the same diff.
 */
export function summaryMessage(summary: EnrichmentSummary): string {
  const parts: string[] = []
  if (summary.fields_added > 0) parts.push(`added ${counted(summary.fields_added, 'field')}`)
  if (summary.fields_overwritten > 0) {
    parts.push(`updated ${counted(summary.fields_overwritten, 'field')}`)
  }
  if (summary.fields_unioned > 0) parts.push(`extended ${counted(summary.fields_unioned, 'list')}`)
  if (summary.identifiers_persisted > 0) {
    parts.push(`linked ${counted(summary.identifiers_persisted, 'identifier')}`)
  }
  const sentence =
    parts.length === 0
      ? 'Applied with no field changes'
      : `${parts[0][0].toUpperCase()}${parts[0].slice(1)}${parts
          .slice(1)
          .map((part) => `, ${part}`)
          .join('')}`
  return summary.cover_stored ? `${sentence}.` : `${sentence}; the cover could not be stored.`
}
