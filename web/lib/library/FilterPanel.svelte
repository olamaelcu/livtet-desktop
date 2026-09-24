<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import type { WorkSortBy } from '../bindings'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys } from '../query/keys'
import { type EditionFilters, type FilterOptions, loadFilterOptions } from '../search'
import {
  FILTER_AXES,
  type FilterAxis,
  normalizeFilters,
  selectedIds,
  setSortBy,
  setSortDirection,
  toggleAxis,
} from './filterAxes'

interface Props {
  filters: EditionFilters
  /** Per-axis search terms, owned by the host so it can clear them on close. */
  searches: Record<string, string>
  onsearch: (searches: Record<string, string>) => void
  onchange: (filters: EditionFilters) => void
  onclose: () => void
}

let { filters, searches, onsearch, onchange, onclose }: Props = $props()

const options = createQuery(() => ({
  queryKey: searchKeys.filterOptions(),
  queryFn: loadFilterOptions,
  staleTime: 5 * 60 * 1000,
}))

function matches(list: { id: string; label: string }[], axis: FilterAxis) {
  const term = (searches[axis] ?? '').trim().toLowerCase()
  return term ? list.filter((option) => option.label.toLowerCase().includes(term)) : list
}

function chooseSortField(event: Event) {
  const value = (event.target as HTMLSelectElement).value
  onchange(setSortBy(filters, value ? (value as WorkSortBy) : undefined))
}

function chooseSortDirection(event: Event) {
  onchange(setSortDirection(filters, (event.target as HTMLSelectElement).value as 'asc' | 'desc'))
}
</script>

<div class="panel" role="group" aria-label="Filters">
  {#if options.isPending}
    <p class="muted">Loading filters…</p>
  {:else if options.isError}
    <p class="muted">Could not load filters.</p>
    <div class="footer">
      <ActionButton variant="brand" onclick={onclose}>Close</ActionButton>
    </div>
  {:else if options.data}
    {#each FILTER_AXES as axis (axis.key)}
      {@const list = axis.from(options.data)}
      <section class="axis">
        <h4>{axis.label}</h4>
        <wa-input
          size="s"
          label="Filter {axis.label}"
          placeholder="Filter {axis.label.toLowerCase()}…"
          value={searches[axis.key] ?? ''}
          oninput={(event) =>
            onsearch({ ...searches, [axis.key]: (event.target as HTMLInputElement).value })}
        ></wa-input>
        <div class="choices">
          {#each matches(list, axis.key) as option (option.id)}
            <label class="choice">
              <input
                type="checkbox"
                checked={selectedIds(filters, axis.key).includes(option.id)}
                onchange={() => onchange(toggleAxis(filters, axis.key, option.id))}
              />
              {option.label}
            </label>
          {/each}
        </div>
      </section>
    {/each}

    <div class="sort">
      <h4>Sort</h4>
      <select aria-label="Sort field" value={filters.sort_by ?? ''} onchange={chooseSortField}>
        <option value="">Relevance</option>
        <option value="created_at">Recently added</option>
        <option value="title">Title</option>
        <option value="updated_at">Recently updated</option>
      </select>
      <select
        aria-label="Sort direction"
        value={filters.sort_direction ?? 'desc'}
        disabled={!filters.sort_by}
        onchange={chooseSortDirection}
      >
        <option value="desc">Descending</option>
        <option value="asc">Ascending</option>
      </select>
    </div>

    <div class="footer">
      <ActionButton onclick={() => onchange(normalizeFilters({}))}>Clear</ActionButton>
      <ActionButton variant="brand" onclick={onclose}>Done</ActionButton>
    </div>
  {/if}
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
    max-height: 28rem;
    overflow-y: auto;
  }

  .axis {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  h4 {
    margin: 0;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-2xs);
  }

  .choice {
    display: flex;
    align-items: center;
    gap: var(--wa-space-3xs);
    font-size: var(--wa-font-size-s);
  }

  .sort {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--wa-space-s);
  }

  .sort h4 {
    flex-basis: 100%;
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-s);
  }

  .muted {
    color: var(--wa-color-text-quiet);
  }
</style>
