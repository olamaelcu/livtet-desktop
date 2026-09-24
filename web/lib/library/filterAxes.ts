import type { SortDirection, WorkSortBy } from '../bindings'
import type { EditionFilters, FilterOptions } from '../search'

/** The seven selectable filter axes (everything in `EditionFilters` except sort). */
export type FilterAxis =
  | 'format_ids'
  | 'language_ids'
  | 'author_ids'
  | 'tag_ids'
  | 'genre_ids'
  | 'subject_ids'
  | 'publisher_ids'

interface FilterAxisConfig {
  key: FilterAxis
  label: string
  from: (options: FilterOptions) => { id: string; label: string }[]
}

/** Ordered axes, shared by the panel (checkboxes) and the page (chip labels). */
export const FILTER_AXES: readonly FilterAxisConfig[] = [
  { key: 'format_ids', label: 'Format', from: (options) => options.formats },
  { key: 'language_ids', label: 'Language', from: (options) => options.languages },
  { key: 'author_ids', label: 'Author', from: (options) => options.authors },
  { key: 'tag_ids', label: 'Tag', from: (options) => options.tags },
  { key: 'genre_ids', label: 'Genre', from: (options) => options.genres },
  { key: 'subject_ids', label: 'Subject', from: (options) => options.subjects },
  { key: 'publisher_ids', label: 'Publisher', from: (options) => options.publishers },
]

/**
 * A mutable view of `EditionFilters`. `EditionFilters` is a serialise/deserialise
 * union, which makes computed-key writes awkward; this flattened shape is
 * structurally compatible with both members.
 */
type FiltersDraft = { [K in FilterAxis]?: string[] } & {
  sort_by?: WorkSortBy | null
  sort_direction?: SortDirection | null
}

export interface FilterChip {
  /** Stable list key: `${axis}:${id}`. */
  key: string
  axis: FilterAxis
  id: string
  label: string
}

export function selectedIds(filters: EditionFilters, axis: FilterAxis): string[] {
  return filters[axis] ?? []
}

/** Drop empty axes, sort each axis' ids, and drop direction when there is no sort field. */
export function normalizeFilters(filters: EditionFilters): EditionFilters {
  const next: FiltersDraft = {}
  for (const axis of FILTER_AXES) {
    const ids = selectedIds(filters, axis.key)
    if (ids.length > 0) next[axis.key] = [...ids].sort()
  }
  if (filters.sort_by) {
    next.sort_by = filters.sort_by
    if (filters.sort_direction) next.sort_direction = filters.sort_direction
  }
  return next
}

export function toggleAxis(filters: EditionFilters, axis: FilterAxis, id: string): EditionFilters {
  const current = selectedIds(filters, axis)
  const ids = current.includes(id) ? current.filter((value) => value !== id) : [...current, id]
  const next: FiltersDraft = { ...filters }
  next[axis] = ids
  return normalizeFilters(next)
}

export function removeAxisId(
  filters: EditionFilters,
  axis: FilterAxis,
  id: string,
): EditionFilters {
  const next: FiltersDraft = { ...filters }
  next[axis] = selectedIds(filters, axis).filter((value) => value !== id)
  return normalizeFilters(next)
}

export function setSortBy(filters: EditionFilters, field: WorkSortBy | undefined): EditionFilters {
  const next: FiltersDraft = { ...filters }
  next.sort_by = field
  if (field && !next.sort_direction) next.sort_direction = 'desc'
  return normalizeFilters(next)
}

export function setSortDirection(
  filters: EditionFilters,
  direction: SortDirection,
): EditionFilters {
  const next: FiltersDraft = { ...filters }
  next.sort_direction = direction
  return normalizeFilters(next)
}

/** Total selected ids across the seven axes (sort is not counted). */
export function activeFilterCount(filters: EditionFilters): number {
  return FILTER_AXES.reduce((total, axis) => total + selectedIds(filters, axis.key).length, 0)
}

/** One chip per selected id, labelled from `options` (falling back to the raw id). */
export function activeChips(filters: EditionFilters, options?: FilterOptions): FilterChip[] {
  const chips: FilterChip[] = []
  for (const axis of FILTER_AXES) {
    for (const id of selectedIds(filters, axis.key)) {
      const label = options
        ? axis.from(options).find((option) => option.id === id)?.label
        : undefined
      chips.push({ key: `${axis.key}:${id}`, axis: axis.key, id, label: label ?? id })
    }
  }
  return chips
}
