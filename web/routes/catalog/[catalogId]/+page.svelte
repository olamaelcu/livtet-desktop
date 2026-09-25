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

const catalogId = $derived(page.params.catalogId ?? '')
const queryClient = useQueryClient()

let href = $state<string | null>(null)
let searchQuery = $state('')
let submitted = $state('')

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

<main class="browser">
  <header class="browser-header">
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
  </header>

  {#if loading}
    <wa-spinner></wa-spinner>
  {:else if failure}
    <wa-callout variant="danger">{opdsErrorMessage(failure)}</wa-callout>
  {:else if active}
    <h2>{active.title}</h2>

    {#if active.navigation.length > 0}
      <nav class="navigation">
        {#each active.navigation as section (section.href ?? section.title)}
          <ActionButton
            disabled={!section.href}
            onclick={() => {
              if (section.href) {
                submitted = ''
                href = section.href
              }
            }}
          >
            {section.title}
          </ActionButton>
        {/each}
      </nav>
    {/if}

    <div class="publications">
      {#each active.publications as publication (publication.identifier ?? publication.title)}
        <wa-card>
          <div class="publication">
            {#if publication.cover_url}
              <img class="cover" src={publication.cover_url} alt="" loading="lazy" />
            {:else}
              <div class="cover placeholder" aria-hidden="true"></div>
            {/if}
            <div class="publication-info">
              <strong>{publication.title}</strong>
              <span class="authors">{publication.authors.join(', ')}</span>
              <ActionButton
                variant="brand"
                disabled={!publication.acquisition_href || acquire.isPending}
                onclick={() =>
                  publication.acquisition_href && acquire.mutate(publication.acquisition_href)}
              >
                Acquire
              </ActionButton>
            </div>
          </div>
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
</main>

<style>
  .browser {
    padding: var(--wa-space-xl);
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .browser-header {
    display: flex;
    align-items: flex-end;
    gap: var(--wa-space-s);
    max-width: 40rem;
  }

  .browser-header wa-input {
    flex: 1 1;
  }

  .navigation {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-s);
  }

  .publications {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: var(--wa-space-m);
  }

  .publication {
    display: flex;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s);
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
  }

  .authors,
  .empty {
    color: var(--wa-color-text-secondary);
    font-size: var(--wa-font-size-xs);
  }

  .pager {
    display: flex;
    justify-content: center;
  }
</style>
