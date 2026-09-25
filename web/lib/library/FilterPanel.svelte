<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import type { Attachment } from 'svelte/attachments'
import type { WorkSortBy } from '../bindings'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys } from '../query/keys'
import { type EditionFilters, loadFilterOptions } from '../search'
import {
  FILTER_AXES,
  type FilterAxis,
  normalizeFilters,
  selectedIds,
  setAxisIds,
  setHasFile,
  setSortBy,
  setSortDirection,
} from './filterAxes'

interface Props {
  filters: EditionFilters
  onchange: (filters: EditionFilters) => void
  onclose: () => void
}

let { filters, onchange, onclose }: Props = $props()

const options = createQuery(() => ({
  queryKey: searchKeys.filterOptions(),
  queryFn: loadFilterOptions,
  staleTime: 5 * 60 * 1000,
}))

function selectValue(event: Event): string {
  const value = (event.currentTarget as WaSelectElement).value
  return typeof value === 'string' ? value : ''
}

function chooseSortField(event: Event) {
  const value = selectValue(event)
  onchange(setSortBy(filters, value ? (value as WorkSortBy) : undefined))
}

function chooseSortDirection(event: Event) {
  onchange(setSortDirection(filters, selectValue(event) === 'asc' ? 'asc' : 'desc'))
}

function availabilityValue(current: EditionFilters): string {
  if (current.has_file === true) return 'true'
  if (current.has_file === false) return 'false'
  return ''
}

function chooseHasFile(event: Event) {
  const value = selectValue(event)
  onchange(setHasFile(filters, value === '' ? null : value === 'true'))
}

function chooseAxisIds(axis: FilterAxis) {
  return (event: Event) => {
    const value = (event.currentTarget as WaSelectElement).value
    onchange(setAxisIds(filters, axis, Array.isArray(value) ? value : [value]))
  }
}

function initials(label: string): string {
  return label
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => word[0])
    .join('')
    .toUpperCase()
}

/**
 * Render a selected option as a removable tag, reusing the option's own
 * `slot="start"` decoration (flag emoji or publisher avatar). Builds elements
 * directly rather than HTML so imported labels can never be injected.
 */
function tagFor(option: WaOptionElement): HTMLElement {
  const tag = document.createElement('wa-tag')
  tag.setAttribute('with-remove', '')
  tag.setAttribute('data-value', option.value)
  const decoration = option.querySelector<HTMLElement>('[slot="start"]')
  if (decoration) {
    tag.append(decoration.cloneNode(true))
    tag.append(document.createTextNode(' '))
  }
  tag.append(document.createTextNode(option.getAttribute('data-label') ?? option.label))
  return tag
}

const decorateTags: Attachment<HTMLElement> = (node) => {
  ;(node as WaSelectElement).getTag = (option) => tagFor(option)
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
    <section class="axis">
      <h4>Availability</h4>
      <wa-select
        size="s"
        class="axis-select"
        aria-label="Availability"
        value={availabilityValue(filters)}
        onchange={chooseHasFile}
      >
        <wa-option value="">Any</wa-option>
        <wa-option value="true">In filesystem</wa-option>
        <wa-option value="false">Not in filesystem</wa-option>
      </wa-select>
    </section>

    {#each FILTER_AXES as axis (axis.key)}
      {@const list = axis.from(options.data)}
      {@const selected = selectedIds(filters, axis.key)}
      <section class="axis">
        <h4>{axis.label}</h4>
        <wa-select
          multiple
          with-clear
          size="s"
          class="axis-select"
          aria-label="Select {axis.label}"
          placeholder="Any {axis.label.toLowerCase()}"
          value={selected}
          onchange={chooseAxisIds(axis.key)}
          {@attach decorateTags}
        >
          {#each list as option (option.id)}
            <wa-option value={option.id} data-label={option.label}>
              {#if option.flag_emoji}
                <span slot="start" class="opt-flag">{option.flag_emoji}</span>
              {:else if option.logo_url}
                <wa-avatar
                  slot="start"
                  image={option.logo_url}
                  initials={initials(option.label)}
                  label={option.label}
                  shape="rounded"
                  loading="lazy"
                  style="--size: 1.25rem;"
                ></wa-avatar>
              {/if}
              {option.label}
            </wa-option>
          {/each}
        </wa-select>
      </section>
    {/each}

    <div class="sort">
      <h4>Sort</h4>
      <wa-select
        size="s"
        class="sort-select"
        aria-label="Sort field"
        value={filters.sort_by ?? ''}
        onchange={chooseSortField}
      >
        <wa-option value="">Relevance</wa-option>
        <wa-option value="created_at">Recently added</wa-option>
        <wa-option value="title">Title</wa-option>
        <wa-option value="updated_at">Recently updated</wa-option>
      </wa-select>
      <wa-select
        size="s"
        class="sort-select"
        aria-label="Sort direction"
        value={filters.sort_direction ?? 'desc'}
        disabled={!filters.sort_by}
        onchange={chooseSortDirection}
      >
        <wa-option value="desc">Descending</wa-option>
        <wa-option value="asc">Ascending</wa-option>
      </wa-select>
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

  .axis-select {
    width: 100%;
  }

  .opt-flag {
    font-size: 1rem;
    line-height: 1;
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

  .sort-select {
    flex: 1 1 8rem;
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
