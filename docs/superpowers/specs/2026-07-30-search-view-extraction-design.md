# Search-view extraction design

**Date:** 2026-07-30
**Scope:** Extract the search page shell into a generic `<SearchView>` component so both `routes/search/+page.svelte` (local catalog) and `routes/search/remote/+page.svelte` (online providers) share the lifecycle, debounce, fetch, error/loading/empty handling, and `<wa-page>` chrome. Filter UI and result filtering remain per-consumer.

## Context

The two search pages duplicate the same shell: a `<wa-page>` with a header containing `<SearchBar>`, a main area with a `<wa-scroller>` of empty/loading/error/result callouts, a debounced `rawQuery → query` effect, and a search effect that resets state on input and kicks off a fetch on the debounced query.

The local page layers format/language facet chips onto that shell; the remote page skips chips and runs an out-of-process search via the `lib/remote/chain.ts` machinery. The shared shape — the shell, the debounce, the state machine, the cover-grid render — is the bulk of the code. Only the data source and the filter UI vary.

Today there is also a latent bug in the local page: the debounce and the search effect are fused into one `$effect`, which reads `rawQuery`, writes `query`, and reads `query` in the same tick. That trips `effect_update_depth_exceeded` under load (flagged in `routes/search/+page.svelte:28-32` with a FIXME). Splitting into two effects is the standard Svelte 5 fix and is folded into this design since the lifecycle is moving into the new component anyway.

## Approach

Extract a single new component — `web/lib/search/components/search-view.svelte` — that owns the shell and the search lifecycle. Consumers inject:

- a data source via a `runSearch(query, limit, onResults)` callback,
- optional `filters`/`result`/`footer` snippets for custom UI,
- messaging strings (`prompt`, `noResults`) for the empty-result callouts.

The local page keeps the format/language filter state, the `deriveFacets`/`filterHits` logic, and the toggle handlers. It passes `filteredHits` into the `result` snippet.

The remote page becomes a thin wrapper around `<SearchView>` plus the existing `runSearch` from `lib/remote/chain.ts` (whose signature already matches the new contract — no changes there).

## New component — `web/lib/search/components/search-view.svelte`

### Props

| Name | Type | Required | Purpose |
|---|---|---|---|
| `title` | `string` | yes | `<svelte:head><title>` |
| `placeholder` | `string` | no (default `"Search the library…"`) | Search input placeholder |
| `runSearch` | `(query: string, limit: number, onResults: (hits: SearchHit[]) => void) => Promise<void>` | yes | Data source. Implementations must cancel any prior in-flight work and invoke `onResults(hits)` exactly once per call. Errors are reported via `onError` (see below). |
| `limit` | `number` | no (default `50`) | Hits per request |
| `prompt` | `string` | yes | Empty-query callout text |
| `noResults` | `string` | yes | Zero-hits callout text (only used when consumer does not override via `result` snippet) |
| `filters` | `Snippet<[{ hits: readonly SearchHit[]; query: string }]>` | no | Filter UI rendered in `<wa-page>`'s `main-header` slot. Receives raw, unfiltered hits and the debounced query so consumers can derive facets. |
| `result` | `Snippet<[{ hits: readonly SearchHit[]; query: string }]>` | no | Result renderer. If omitted, raw hits render directly into `<CoverGrid>`. If provided, the consumer is responsible for everything inside the scroller (including filtered "no matches" messaging). |
| `footer` | `Snippet<[{ count: number }]>` | no | Footer content rendered in `<wa-page>`'s `main-footer` slot (e.g. result count). Receives the raw hit count. |
| `onError` | `(error: string) => void` | no | Called by `runSearch` (via a second parameter — see signature below) when the fetch fails. When omitted, errors fall through to `onResults([])` and the warning callout is skipped. |

### Implementation notes

- **Component-owned state**: `rawQuery: string`, `query: string`, `allHits: readonly SearchHit[]`, `loading: boolean`, `searchError: string | null`.
- **`runSearch` signature** (revised): `(query, limit, onResults, onError?) => Promise<void>`. The component passes both callbacks; `runSearch` implementations in `routes/search/+page.svelte` and `lib/remote/chain.ts` are updated to call `onError(msg)` on failure. `chain.ts`'s `runSearch` currently folds errors into `onResults([])` — that's preserved as the fallback when `onError` is not provided.
- **Effects split into two** (fixes the existing `effect_update_depth_exceeded` FIXME):
  1. Debounce: reads `rawQuery`, schedules `query = rawQuery` after 150ms, returns `clearTimeout` cleanup.
  2. Search: reads `query.trim()`. Empty → clears `allHits` and `searchError`. Non-empty → sets `loading = true`, calls `runSearch`, routes the result through `onResults` or `onError`. Returns cleanup that cancels in-flight work via a `cancelled` flag.

  Two effects, not one. The first effect reads only `rawQuery`; the second reads only `query`. No effect reads and writes the same piece of state.

### Render skeleton

```svelte
<svelte:head><title>{title}</title></svelte:head>

<wa-page>
  <header slot="header">
    <SearchBar bind:value={rawQuery} {placeholder} />
  </header>

  {#if filters}
    <div slot="main-header">
      {@render filters({ hits: allHits, query })}
    </div>
  {/if}

  <main>
    <wa-scroller orientation="vertical" class="result-scroller">
      {#if searchError}
        <wa-callout variant="warning">
          <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
          Search failed: {searchError}
        </wa-callout>
      {:else if query.trim() === ""}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          {prompt}
        </wa-callout>
      {:else if loading && allHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="hourglass"></wa-icon>
          Searching…
        </wa-callout>
      {:else if result}
        {@render result({ hits: allHits, query })}
      {:else if allHits.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          {noResults}
        </wa-callout>
      {:else}
        <CoverGrid hits={allHits} />
      {/if}
    </wa-scroller>
  </main>

  {#if footer}
    <footer slot="main-footer">
      {@render footer({ count: allHits.length })}
    </footer>
  {/if}
</wa-page>
```

`SearchView` uses the same CSS as today: `.result-scroller { max-height: calc(100vh - 20rem); }`, `.facet-row`, `.facet-label`, `.result-count`. These move into the new component's `<style>` block; the local page keeps only the bits it owns (none today).

## Consumer 1 — `routes/search/+page.svelte`

Drops the IPC effect, debounce, and lifecycle (~50 lines). Keeps `FilterState`, `deriveFacets`, `filterHits`, `toggleFormat`, `toggleLanguage`, `<CommandScope id="search">`, and the format/language filter UI — now in a `filters` snippet.

```svelte
<script lang="ts">
  import { deriveFacets } from "$lib/search/deriveFacets";
  import { filterHits } from "$lib/search/search";
  import { emptyFilters, type FilterState } from "$lib/search/types";
  import { commands } from "$lib/bindings";
  import SearchView from "$lib/search/components/search-view.svelte";
  import FilterChip from "$lib/search/components/filter-chip.svelte";
  import CoverGrid from "$lib/search/components/cover-grid.svelte";
  import CommandScope from "$lib/commands/components/command-scope.svelte";

  const SEARCH_LIMIT = 50;

  let filters: FilterState = $state(emptyFilters());

  function toggleFormat(label: string) {
    const next = new Set(filters.formats);
    if (next.has(label)) next.delete(label); else next.add(label);
    filters = { ...filters, formats: next };
  }

  function toggleLanguage(label: string) {
    const next = new Set(filters.languages);
    if (next.has(label)) next.delete(label); else next.add(label);
    filters = { ...filters, languages: next };
  }
</script>

<CommandScope id="search">
  <SearchView
    title="Search · livtet"
    limit={SEARCH_LIMIT}
    runSearch={(q, limit, onResults, onError) =>
      commands.search(q, limit)
        .then((r) => {
          if (r.status === "ok") onResults(r.data);
          else { onError?.(r.error); onResults([]); }
        })
        .catch((e) => { onError?.(String(e)); onResults([]); })}
    prompt="Type a query to search the catalog."
    noResults="No matches."
  >
    {#snippet filters({ hits, query })}
      {@const facets = deriveFacets(hits)}
      {#if facets.formats.length > 0}
        <section class="facet-row" aria-label="Format filter">
          <span class="facet-label">Format</span>
          {#if filters.formats.size > 0}
            <wa-badge variant="brand" appearance="filled">
              {filters.formats.size}
            </wa-badge>
          {/if}
          {#each facets.formats as label (label)}
            <FilterChip
              id={`format-${label}`}
              {label}
              selected={filters.formats.has(label)}
              ontoggle={() => toggleFormat(label)}
            />
          {/each}
        </section>
      {/if}

      {#if facets.languages.length > 0}
        <section class="facet-row" aria-label="Language filter">
          <span class="facet-label">Language</span>
          {#if filters.languages.size > 0}
            <wa-badge variant="brand" appearance="filled">
              {filters.languages.size}
            </wa-badge>
          {/if}
          {#each facets.languages as label (label)}
            <FilterChip
              id={`language-${label}`}
              {label}
              selected={filters.languages.has(label)}
              ontoggle={() => toggleLanguage(label)}
            />
          {/each}
        </section>
      {/if}
    {/snippet}

    {#snippet result({ hits, query })}
      {@const filtered = filterHits(query, filters, hits)}
      {#if filtered.length === 0}
        <wa-callout variant="neutral">
          <wa-icon slot="icon" name="circle-info"></wa-icon>
          No matches for "{query}". Try fewer filters or a shorter query.
        </wa-callout>
      {:else}
        <CoverGrid hits={filtered} />
      {/if}
    {/snippet}

    {#snippet footer({ count })}
      <p class="result-count">
        {count} {count === 1 ? "result" : "results"}
      </p>
    {/snippet}
  </SearchView>
</CommandScope>
```

## Consumer 2 — `routes/search/remote/+page.svelte`

Drops the debounce/effect block (~10 lines). Becomes a thin wrapper around `<SearchView>` plus `runSearch` from `lib/remote/chain.ts`.

```svelte
<script lang="ts">
  import SearchView from "$lib/search/components/search-view.svelte";
  import { runSearch } from "$lib/remote/chain";

  const LIMIT = 50;
</script>

<svelte:head><title>Search Online · livtet</title></svelte:head>

<SearchView
  title="Search Online · livtet"
  placeholder="Search Hardcover, Google Books, OpenLibrary…"
  limit={LIMIT}
  runSearch={runSearch}
  prompt="Type a query to search online catalogs."
  noResults="No results across Google Books, Hardcover, or OpenLibrary."
/>
```

`runSearch` in `lib/remote/chain.ts` already conforms to the `(q, limit, onResults, onError?) => Promise<void>` shape — no changes needed.

## Files touched

| File | Change |
|---|---|
| `web/lib/search/components/search-view.svelte` | **new** |
| `web/routes/search/+page.svelte` | rewrite to consume `<SearchView>`; drop the inline IPC + debounce effect |
| `web/routes/search/remote/+page.svelte` | rewrite to consume `<SearchView>`; drop the debounce/effect block |
| `web/lib/search/deriveFacets.ts` | untouched |
| `web/lib/search/search.ts` | untouched |
| `web/lib/search/types.ts` | untouched |
| `web/lib/search/components/cover-grid.svelte` | untouched |
| `web/lib/search/components/filter-chip.svelte` | untouched |
| `web/lib/search/components/search-bar.svelte` | untouched |
| `web/lib/remote/chain.ts` | untouched (signature already fits) |

## Verification

1. **Type check**: `cargo check -p livtet-desktop` and Svelte/TS type checking (project-default command) — must pass.
2. **Bindings unchanged**: `cargo run -p livtet-desktop --bin generate-bindings && git diff web/lib/bindings.ts` — empty diff (no Tauri commands touched).
3. **Local search smoke test** (`pnpm tauri dev`, navigate to `/search`):
   - Type a query → debounced 150ms → results appear in `<CoverGrid>`.
   - Toggle a format chip → grid filters immediately.
   - Toggle a language chip → grid filters immediately.
   - Clear input → "Type a query to search the catalog." callout.
   - Search for gibberish → "No matches for ..." callout (which itself is the consumer's filtered no-matches message).
   - Verify no `effect_update_depth_exceeded` errors in the dev console.
4. **Remote search smoke test** (navigate to `/search/remote`):
   - Type a query → debounced → chain runs → results stream in.
   - No filter chips visible (filter slot empty).
   - Clear input → "Type a query to search online catalogs." callout.
   - Search for gibberish → "No results across Google Books..." callout (component's default).
5. **Audit**: `git diff --stat` should show only the three files listed above.

## Non-goals

- No changes to the data shapes (`SearchHitRow`, `FilterState`).
- No changes to `lib/remote/chain.ts` — signature already matches.
- No changes to the cover grid, filter chip, or search bar components.
- No new abstraction for facet derivation — it stays in `lib/search/deriveFacets.ts` and is invoked from the consumer's `filters` snippet.
