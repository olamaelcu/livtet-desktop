<script lang="ts">
import { createMutation, createQuery, useQueryClient } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { page } from '$app/state'
import ActionButton from '../../../lib/components/ActionButton.svelte'
import {
  acquireItem,
  loadFeed,
  loadPage,
  type OpdsFeed,
  opdsErrorMessage,
  searchCatalog,
} from '../../../lib/opds'
import { opdsKeys, searchKeys } from '../../../lib/query/keys'
import CatalogDetailLayout from './CatalogDetailLayout.svelte'

const catalogId = $derived(page.params.catalogId ?? '')
const queryClient = useQueryClient()

let href = $state<string | null>(null)
let searchQuery = $state('')
let submitted = $state('')
let history = $state<Array<{ title: string; href: string }>>([])

const feed = createQuery(() => ({
  queryKey: [...opdsKeys.feed(catalogId), href] as const,
  queryFn: () => (href ? loadPage(catalogId, href) : loadFeed(catalogId)),
  enabled: catalogId.length > 0,
  staleTime: 60_000,
  retry: 1,
}))

const results = createQuery(() => ({
  queryKey: opdsKeys.search(catalogId, submitted),
  queryFn: () => searchCatalog(catalogId, submitted),
  enabled: catalogId.length > 0 && submitted.length > 0,
  staleTime: 60_000,
  retry: 1,
}))

const active: OpdsFeed | undefined = $derived(submitted ? results.data : feed.data)
const loading = $derived(submitted ? results.isPending : feed.isPending)
const failure = $derived(submitted ? results.error : feed.error)

function reset() {
  href = null
  submitted = ''
  searchQuery = ''
  history = []
}

function runSearch() {
  const query = searchQuery.trim()
  if (!query) return
  href = null
  submitted = query
}

const acquire = createMutation(() => ({
  mutationFn: (url: string) => acquireItem(catalogId, url),
  onSuccess: (outcome) => {
    toast.success(
      outcome.duplicate ? `Already in library: ${outcome.title}` : `Imported ${outcome.title}`,
    )
    queryClient.invalidateQueries({ queryKey: searchKeys.all })
  },
  onError: (error) => toast.error(opdsErrorMessage(error)),
}))
</script>

<CatalogDetailLayout>
  {#snippet toolbar()}
    {#if active?.has_search || submitted}
      <wa-input
        placeholder="Search this catalog"
        oninput={(event) => (searchQuery = event.currentTarget.value)}
      ></wa-input>
      <ActionButton onclick={runSearch} variant="brand">Search</ActionButton>
    {/if}
    {#if href || submitted}
      <ActionButton onclick={reset}>Back to root</ActionButton>
    {/if}
  {/snippet}

  {#if loading}
    <wa-spinner></wa-spinner>
  {:else if failure}
    <wa-callout variant="danger">{opdsErrorMessage(failure)}</wa-callout>
  {:else if active}
    <h2>{active.title}</h2>
    <nav class="breadcrumb" aria-label="Path" data-testid="breadcrumb">
      <a href="#" onclick={(e) => { e.preventDefault(); href = null; submitted = ''; history = []; }} class="crumb" data-testid="breadcrumb-root">Root</a>
      {#each history as entry, i (entry.href)}
        <span aria-hidden="true" class="sep">›</span>
        <a href="#" onclick={(e) => { e.preventDefault(); href = entry.href; history = history.slice(0, i + 1); }} class="crumb" data-testid={`breadcrumb-${i}`}>{entry.title}</a>
      {/each}
      {#if href}
        <span aria-hidden="true" class="sep">›</span>
        <span class="crumb current" data-testid="breadcrumb-current">{active.title}</span>
      {/if}
    </nav>

    {#if active.navigation.length > 0}
      <section class="folder-list" aria-label="Folders" data-testid="folder-list">
        {#each active.navigation as section (section.href ?? section.title)}
          <a
            class="folder-row"
            href="#"
            data-testid="folder-link"
            onclick={(e) => { e.preventDefault(); if (section.href) { submitted = ''; href = section.href; history = [...history, { title: section.title, href: section.href }]; } }}
            aria-disabled={!section.href}
          >
            <wa-icon name="folder"></wa-icon>
            <span class="folder-title">{section.title}</span>
            <wa-icon name="chevron-right" class="folder-arrow"></wa-icon>
          </a>
        {/each}
      </section>
    {/if}

    <div class="publications">
      {#each active.publications as publication (publication.identifier ?? publication.title)}
        <wa-card>
          <a href="#" onclick={(e) => { e.preventDefault(); publication.acquisition_href && acquire.mutate(publication.acquisition_href); }} class="publication-link" aria-label={publication.title}>
            <div class="publication">
              {#if publication.cover_url}
                <img class="cover" src={publication.cover_url} alt="" loading="lazy" />
              {:else}
                <div class="cover placeholder" aria-hidden="true"></div>
              {/if}
              <div class="publication-info">
                <strong class="pub-title">{publication.title}</strong>
                <div class="pub-meta">{publication.authors.join(', ')}</div>
                {#if publication.summary}
                  <div class="pub-summary">{publication.summary}</div>
                {/if}
                <div class="pub-tags">
                  {#if publication.language}<span class="tag">{publication.language}</span>{/if}
                  {#if publication.publisher}<span class="tag">{publication.publisher}</span>{/if}
                  {#if publication.published}<span class="tag">{publication.published}</span>{/if}
                </div>
                {#if publication.acquisition_href}
                  <wa-button size="small" onclick={() => acquire.mutate(publication.acquisition_href!)} disabled={acquire.isPending}>Acquire</wa-button>
                {/if}
              </div>
            </div>
          </a>
        </wa-card>
      {/each}
    </div>

    {#if active.publications.length === 0 && active.navigation.length === 0}
      <p class="empty">This feed has no publications.</p>
    {/if}

    {#if active.next_href}
      <div class="pager">
        <ActionButton onclick={() => (href = active?.next_href ?? null)}>Next page</ActionButton>
      </div>
    {/if}
  {/if}
</CatalogDetailLayout>

<style>
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs);
  }

  .crumb {
    color: var(--wa-color-text-secondary);
    text-decoration: none;
  }

  .crumb.current {
    color: var(--wa-color-text-primary);
    font-weight: 600;
  }

  .sep {
    color: var(--wa-color-text-secondary);
    font-size: 0.7rem;
  }

  .folder-list {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    border: var(--wa-border-width-s) var(--wa-border-style) var(--wa-color-surface-border);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-container-high, var(--wa-color-surface-alt));
  }

  .folder-row {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-m);
    color: inherit;
    text-decoration: none;
    transition: background 0.15s ease;
  }

  .folder-row:hover {
    background: var(--wa-color-surface-hover, color-mix(in srgb, var(--wa-color-surface-alt) 30%, transparent));
  }

  .folder-row[aria-disabled='true'] {
    opacity: 0.5;
    pointer-events: none;
  }

  .folder-icon {
    font-size: 1.1rem;
    flex: none;
  }

  .folder-title {
    flex: 1;
    font-weight: 500;
  }

  .folder-arrow {
    flex: none;
    font-size: 1.1rem;
    color: var(--wa-color-text-secondary);
  }

  .publications {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: var(--wa-space-m);
  }

  .publication-link {
    color: inherit;
    text-decoration: none;
  }

  .publication {
    display: flex;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s);
    height: 100%;
  }

  .cover {
    width: 4.5rem;
    height: 6.75rem;
    object-fit: cover;
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-alt);
    flex: none;
  }

  .publication-info {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-2xs);
    min-width: 0;
    flex: 1;
    height: 100%;
    justify-content: space-between;
  }

  .pub-title {
    font-size: var(--wa-font-size-sm);
  }

  .pub-meta {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }

  .pub-summary {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .pub-tags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-2xs);
  }

  .tag {
    display: inline-block;
    background: var(--wa-color-surface-alt);
    padding: 0 var(--wa-space-2xs);
    border-radius: calc(var(--wa-border-radius) / 2);
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-secondary);
  }

  .empty {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }

  .pager {
    display: flex;
    justify-content: center;
  }
</style>
