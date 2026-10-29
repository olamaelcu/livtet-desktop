<script lang="ts">
import { useQueryClient } from '@tanstack/svelte-query'
import { invoke } from '@tauri-apps/api/core'
import { toast } from 'svelte-sonner'
import ActionButton from '../components/ActionButton.svelte'
import { searchKeys } from '../query/keys'
import { loadFilterOptions } from '../search'

interface Props {
  open: boolean
  onclose: () => void
}

let { open, onclose }: Props = $props()

interface VirtualAuthor {
  name: string
  role: string
}

interface VirtualBookInput {
  title: string
  authors: VirtualAuthor[]
  publisher: string | null
  language_id: string | null
  format_id: string | null
  identifiers: string[]
  description: string | null
  notes: string | null
  cover_image_url: string | null
}

/** One edition summary from the book index (`search_biblio_editions`). */
interface BiblioEdition {
  uri: string | null
  title: string
  cover_image_url: string | null
  published_year: number | null
  language: string | null
  authors: VirtualAuthor[]
  identifiers: { resource: string; uri: string }[]
}

interface Option {
  id: string
  label: string
}

/** MARC relator codes the rest of the catalog uses for contributors. */
const ROLE_OPTIONS = [
  { value: 'aut', label: 'Author' },
  { value: 'edt', label: 'Editor' },
  { value: 'trl', label: 'Translator' },
]
const SEARCH_LIMIT = 20

const queryClient = useQueryClient()

// Manual entry state.
let title = $state('')
let authors = $state<VirtualAuthor[]>([{ name: '', role: 'aut' }])
let publisher = $state('')
let languageId = $state('')
let formatId = $state('')
let identifiers = $state('')
let description = $state('')
let notes = $state('')
let coverUrl = $state('')
let formatOptions = $state<Option[]>([])
let languageOptions = $state<Option[]>([])

// Book-index search state.
let searchInput = $state('')
let searching = $state(false)
let searchError = $state<string | null>(null)
let results = $state<BiblioEdition[]>([])
let searchedOnce = $state(false)

let busy = $state(false)

function eventValue(event: Event): string {
  return (event.currentTarget as HTMLElement & { value: string }).value ?? ''
}

// Filter options drive the manual language and format pickers. Loaded each time
// the drawer opens so a newly seeded format shows up without a reload.
$effect(() => {
  if (!open) return
  let cancelled = false
  loadFilterOptions()
    .then((options) => {
      if (cancelled) return
      formatOptions = options.formats.map((format) => ({ id: format.id, label: format.label }))
      languageOptions = options.languages.map((language) => ({
        id: language.id,
        label: language.label,
      }))
    })
    .catch(() => undefined)
  return () => {
    cancelled = true
  }
})

const ready = $derived(
  title.trim().length > 0 && authors.some((author) => author.name.trim().length > 0) && !busy,
)

function addAuthor() {
  authors = [...authors, { name: '', role: 'aut' }]
}

function removeAuthor(index: number) {
  const next = authors.filter((_, position) => position !== index)
  authors = next.length > 0 ? next : [{ name: '', role: 'aut' }]
}

function setAuthorName(index: number, name: string) {
  authors = authors.map((author, position) => (position === index ? { ...author, name } : author))
}

function setAuthorRole(index: number, role: string) {
  authors = authors.map((author, position) => (position === index ? { ...author, role } : author))
}

function reset() {
  title = ''
  authors = [{ name: '', role: 'aut' }]
  publisher = ''
  languageId = ''
  formatId = ''
  identifiers = ''
  description = ''
  notes = ''
  coverUrl = ''
  searchInput = ''
  searchError = null
  results = []
  searchedOnce = false
}

async function submit(input: VirtualBookInput) {
  busy = true
  try {
    await invoke('create_virtual_book', { input })
    await queryClient.invalidateQueries({ queryKey: searchKeys.all })
    toast.success(`Added “${input.title}” as a virtual book`)
    reset()
    onclose()
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not create the virtual book')
  } finally {
    busy = false
  }
}

async function createManual() {
  if (!ready) return
  await submit({
    title: title.trim(),
    authors: authors
      .filter((author) => author.name.trim().length > 0)
      .map((author) => ({ name: author.name.trim(), role: author.role })),
    publisher: publisher.trim() || null,
    language_id: languageId || null,
    format_id: formatId || null,
    identifiers: identifiers
      .split(/[\n,]/)
      .map((value) => value.trim())
      .filter((value) => value.length > 0),
    description: description.trim() || null,
    notes: notes.trim() || null,
    cover_image_url: coverUrl.trim() || null,
  })
}

async function search() {
  const query = searchInput.trim()
  if (query.length === 0 || searching) return
  searching = true
  searchError = null
  searchedOnce = true
  try {
    results = await invoke<BiblioEdition[]>('search_biblio_editions', {
      query,
      limit: SEARCH_LIMIT,
    })
  } catch (error) {
    results = []
    searchError = error instanceof Error ? error.message : 'Could not search the book index'
  } finally {
    searching = false
  }
}

async function addFromIndex(edition: BiblioEdition) {
  await submit({
    title: edition.title,
    authors: edition.authors.map((author) => ({ name: author.name, role: author.role })),
    publisher: null,
    language_id: null,
    format_id: null,
    identifiers: edition.identifiers.map((identifier) => identifier.uri),
    description: null,
    notes: null,
    cover_image_url: edition.cover_image_url,
  })
}

function submitSearch(event: SubmitEvent) {
  event.preventDefault()
  void search()
}
</script>

<wa-drawer
  label="Add a virtual book"
  placement="end"
  open={open}
  onwa-after-hide={onclose}
>
  <wa-tab-group>
    <wa-tab slot="nav" panel="manual">Manual entry</wa-tab>
    <wa-tab slot="nav" panel="search">Search online</wa-tab>

    <wa-tab-panel name="manual">
      <div class="panel">
        <p class="summary">
          A virtual book lives in the catalog only — no file is stored, so it cannot be read.
        </p>

        <wa-input
          label="Title"
          required
          value={title}
          oninput={(event: Event) => (title = eventValue(event))}
        ></wa-input>

        <wa-input
          label="Cover image URL"
          placeholder="https://…"
          value={coverUrl}
          oninput={(event: Event) => (coverUrl = eventValue(event))}
        ></wa-input>

        <fieldset class="authors">
          <legend>Authors</legend>
          {#each authors as author, index (index)}
            <div class="author-row">
              <wa-input
                label="Name"
                value={author.name}
                oninput={(event: Event) => setAuthorName(index, eventValue(event))}
              ></wa-input>
              <wa-select
                label="Role"
                value={author.role}
                onchange={(event: Event) => setAuthorRole(index, eventValue(event))}
              >
                {#each ROLE_OPTIONS as option (option.value)}
                  <wa-option value={option.value}>{option.label}</wa-option>
                {/each}
              </wa-select>
              <ActionButton
                size="small"
                onclick={() => removeAuthor(index)}
                disabled={authors.length === 1}
              >
                <wa-icon name="xmark"></wa-icon>
              </ActionButton>
            </div>
          {/each}
          <ActionButton size="small" onclick={addAuthor}>
            <wa-icon name="plus"></wa-icon>
            Add author
          </ActionButton>
        </fieldset>

        <wa-input
          label="Publisher"
          value={publisher}
          oninput={(event: Event) => (publisher = eventValue(event))}
        ></wa-input>

        <wa-select
          label="Language"
          value={languageId}
          onchange={(event: Event) => (languageId = eventValue(event))}
        >
          <wa-option value="">None</wa-option>
          {#each languageOptions as option (option.id)}
            <wa-option value={option.id}>{option.label}</wa-option>
          {/each}
        </wa-select>

        <wa-select
          label="Format"
          value={formatId}
          onchange={(event: Event) => (formatId = eventValue(event))}
        >
          <wa-option value="">None</wa-option>
          {#each formatOptions as option (option.id)}
            <wa-option value={option.id}>{option.label}</wa-option>
          {/each}
        </wa-select>

        <wa-textarea
          label="Identifiers"
          placeholder="One per line or comma-separated (ISBN, ASIN, …)"
          value={identifiers}
          oninput={(event: Event) => (identifiers = eventValue(event))}
        ></wa-textarea>

        <wa-textarea
          label="Description"
          value={description}
          oninput={(event: Event) => (description = eventValue(event))}
        ></wa-textarea>

        <wa-textarea
          label="Notes"
          value={notes}
          oninput={(event: Event) => (notes = eventValue(event))}
        ></wa-textarea>

        <div class="actions">
          <ActionButton onclick={onclose} disabled={busy}>Cancel</ActionButton>
          <ActionButton variant="brand" onclick={createManual} disabled={!ready}>
            {busy ? 'Creating…' : 'Create virtual book'}
          </ActionButton>
        </div>
      </div>
    </wa-tab-panel>

    <wa-tab-panel name="search">
      <div class="panel">
        <p class="summary">
          Search the book index for an edition, then add it as a virtual book.
        </p>
        <form class="search-row" onsubmit={submitSearch}>
          <wa-input
            placeholder="Title, author, …"
            value={searchInput}
            oninput={(event: Event) => (searchInput = eventValue(event))}
          ></wa-input>
          <ActionButton
            variant="brand"
            disabled={searching || searchInput.trim().length === 0}
            onclick={() => void search()}
          >
            {searching ? 'Searching…' : 'Search'}
          </ActionButton>
        </form>

        {#if searchError}
          <wa-callout variant="danger">{searchError}</wa-callout>
        {:else if searching}
          <p class="muted">Searching the book index…</p>
        {:else if results.length > 0}
          <ul class="results">
            {#each results as edition, index (edition.uri ?? `${edition.title}-${index}`)}
              <li class="result">
                {#if edition.cover_image_url}
                  <img
                    class="result-cover"
                    src={edition.cover_image_url}
                    alt=""
                    loading="lazy"
                    onerror={(event) => {
                      ;(event.currentTarget as HTMLImageElement).style.display = 'none'
                    }}
                  />
                {/if}
                <div class="result-body">
                  <span class="result-title">{edition.title}</span>
                  {#if edition.published_year}
                    <span class="muted small">{edition.published_year}</span>
                  {/if}
                  {#if edition.authors.length > 0}
                    <span class="muted small">
                      {edition.authors.map((author) => author.name).join(', ')}
                    </span>
                  {/if}
                  {#if edition.identifiers.length > 0}
                    <span class="result-ids">
                      {#each edition.identifiers as identifier (identifier.uri)}
                        <wa-badge>{identifier.resource}</wa-badge>
                      {/each}
                    </span>
                  {/if}
                </div>
                <ActionButton disabled={busy} onclick={() => addFromIndex(edition)}>
                  Add
                </ActionButton>
              </li>
            {/each}
          </ul>
        {:else if searchedOnce}
          <p class="muted">No editions matched that search.</p>
        {/if}
      </div>
    </wa-tab-panel>
  </wa-tab-group>
</wa-drawer>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
    padding-top: var(--wa-space-m);
  }

  .summary {
    margin: 0;
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
  }

  .authors {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    margin: 0;
    padding: 0;
    border: none;
  }

  .authors legend {
    padding: 0;
    font-weight: var(--wa-font-weight-semibold);
  }

  .author-row {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: end;
    gap: var(--wa-space-s);
  }

  .search-row {
    display: flex;
    align-items: end;
    gap: var(--wa-space-s);
  }

  .results {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xs);
    margin: 0;
    padding: 0;
    max-height: 28rem;
    overflow-y: auto;
    list-style: none;
  }

  .result {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .result:first-of-type {
    border-top: none;
  }

  .result-cover {
    width: 2.5rem;
    height: 3.5rem;
    object-fit: cover;
    border-radius: var(--wa-border-radius);
    flex-shrink: 0;
  }

  .result-body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    gap: 0.125rem;
  }

  .result-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: var(--wa-font-weight-semibold);
  }

  .result-ids {
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-3xs);
  }

  .small {
    font-size: var(--wa-font-size-xs);
  }

  .muted {
    color: var(--wa-color-text-secondary);
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--wa-space-s);
  }
</style>
