<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { catalogKeys } from '../query/keys'
import { coverUrlFor, loadEditionDetail } from '../search'
import { fileName, formatFileSize } from './format'

interface Props {
  editionId: string | null
  open: boolean
  onclose: () => void
}

let { editionId, open, onclose }: Props = $props()

const detail = createQuery(() => ({
  queryKey: catalogKeys.editionDetail(editionId ?? ''),
  queryFn: async () => (editionId ? loadEditionDetail(editionId) : null),
  enabled: open && editionId !== null,
}))

const book = $derived(detail.data ?? null)
const coverUrl = $derived(coverUrlFor(book?.file?.cover_path))
let failedCoverId = $state<string | null>(null)
const showCover = $derived(coverUrl !== undefined && failedCoverId !== book?.id)

onMount(() => {
  console.log(book?.description)
})
</script>

<wa-drawer label="Edition detail" placement="end" open={open} onwa-after-hide={onclose}>
  <div class="body">
    {#if detail.isPending}
      <p class="muted">Loading…</p>
    {:else if detail.isError}
      <p class="error">Could not load this book's details.</p>
    {:else if book}
      <header class="header">
        <figure class="cover">
          {#if showCover}
            <img
              src={coverUrl}
              alt={book.title ?? 'Cover'}
              onerror={() => (failedCoverId = book?.id ?? '')}
            />
          {:else}
            <figcaption class="placeholder">{book.title ?? 'Untitled'}</figcaption>
          {/if}
        </figure>
        <div class="heading">
          <h2 class="title">{book.title ?? 'Untitled'}</h2>
          {#if book.authors.length > 0}
            <ul class="authors">
              {#each book.authors as author (author.name + author.role)}
                <li class="author">
                  <span>{author.name}</span>
                  <wa-badge>{author.role}</wa-badge>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </header>

      <dl class="facts">
        {#if book.format}
          <div class="fact"><dt>Format</dt><dd>{book.format}</dd></div>
        {/if}
        {#if book.published_date}
          <div class="fact"><dt>Published</dt><dd>{book.published_date}</dd></div>
        {/if}
        {#if book.language_code}
          <div class="fact"><dt>Language</dt><dd>{book.language_code}</dd></div>
        {/if}
        {#if book.publishers.length > 0}
          <div class="fact"><dt>Publisher</dt><dd>{book.publishers.join(', ')}</dd></div>
        {/if}
      </dl>

      {#if book.identifiers.length > 0}
        <section class="section">
          <h3 class="section-title">Identifiers</h3>
          <ul class="identifiers">
            {#each book.identifiers as identifier (identifier.value)}
              <li class="identifier">
                <wa-badge>{identifier.kind}</wa-badge>
                <span class="mono">{identifier.value}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if book.file}
        {@const file = book.file}
        <section class="section">
          <h3 class="section-title">File</h3>
          <dl class="facts">
            {#if file.file_format}
              <div class="fact"><dt>Format</dt><dd>{file.file_format}</dd></div>
            {/if}
            {#if file.file_size_bytes !== null}
              {@const size = file.file_size_bytes}
              <div class="fact">
                <dt>Size</dt>
                <dd>{formatFileSize(size)}</dd>
              </div>
            {/if}
            {#if file.file_path}
              {@const path = file.file_path}
              <div class="fact">
                <dt>Path</dt>
                <dd class="mono" title={path}>{fileName(path)}</dd>
              </div>
            {/if}
          </dl>
        </section>
      {/if}

      {#if book.description}
        <section class="section">
          <h3 class="section-title">Description</h3>
          <p>{@html book.description}</p>
        </section>
      {/if}

      {#if book.notes}
        <section class="section">
          <h3 class="section-title">Notes</h3>
          <p>{book.notes}</p>
        </section>
      {/if}
    {:else}
      <p class="muted">No catalog record found for this book.</p>
    {/if}
  </div>
</wa-drawer>

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-l);
  }

  .header {
    display: flex;
    gap: var(--wa-space-l);
    align-items: flex-start;
  }

  .cover {
    --size: 7rem;
    width: var(--size);
    min-width: var(--size);
    height: calc(var(--size) * 1.5);
    margin: 0;
    overflow: hidden;
    border: 0.0625rem solid var(--wa-color-border-default);
    border-radius: var(--wa-border-radius);
    background: var(--wa-color-surface-alt);
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--wa-space-s);
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-secondary);
    text-align: center;
  }

  .heading {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    min-width: 0;
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-l);
  }

  .authors {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .author {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: 0;
  }

  .fact {
    display: flex;
    justify-content: space-between;
    gap: var(--wa-space-m);
  }

  .fact dt {
    color: var(--wa-color-text-secondary);
  }

  .fact dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .section-title {
    margin: 0;
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .section p {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .identifiers {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .identifier {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .mono {
    font-family: var(--wa-font-family-mono, monospace);
    overflow-wrap: anywhere;
  }

  .muted {
    color: var(--wa-color-text-secondary);
  }

  .error {
    color: var(--wa-color-danger);
  }
</style>
