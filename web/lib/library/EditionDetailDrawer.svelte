<script lang="ts">
import { createQuery, useQueryClient } from '@tanstack/svelte-query'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { toast } from 'svelte-sonner'
import ActionButton from '../components/ActionButton.svelte'
import { catalogKeys } from '../query/keys'
import { openReader } from '../reader'
import { coverUrlFor, loadEditionDetail } from '../search'
import { fileName, formatFileSize, formatIdentifier } from './format'
import { relinkEditionFile } from './import'

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

const queryClient = useQueryClient()
const FILE_FILTERS = [{ name: 'Books', extensions: ['epub', 'azw3', 'azw', 'pdf', 'm4b', 'm4a'] }]
let relinking = $state(false)
let listening = $state(false)

const isAudiobook = $derived(book?.format?.toLowerCase() === 'audiobook')
const canListen = $derived(isAudiobook && book?.file != null && book.file.file_status !== 'missing')

async function listenBook() {
  if (!editionId || listening) return
  try {
    listening = true
    await openReader(editionId)
    onclose()
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not open the reader')
  } finally {
    listening = false
  }
}

async function relinkFile() {
  if (!editionId || relinking) return
  try {
    const picked = await openDialog({ multiple: false, filters: FILE_FILTERS })
    if (!picked || Array.isArray(picked)) return
    relinking = true
    await relinkEditionFile(editionId, picked)
    await queryClient.invalidateQueries({ queryKey: catalogKeys.editionDetail(editionId) })
    toast.success('File re-linked')
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not re-link the file')
  } finally {
    relinking = false
  }
}

async function revealFile(path: string) {
  try {
    await revealItemInDir(path)
  } catch (error) {
    toast.error(error instanceof Error ? error.message : 'Could not reveal the file')
  }
}
</script>

<wa-drawer class="drawer" label="Edition detail" placement="end" open={open} onwa-after-hide={onclose}>
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
            <figcaption class="detail-placeholder">{book.title ?? 'Untitled'}</figcaption>
          {/if}
        </figure>
        <div class="heading">
          <h2 class="title">{book.title ?? 'Untitled'}</h2>
          {#if book.authors.length > 0}
            <ul class="authors">
              {#each book.authors as author (author.name + author.role)}
                <li class="author">
                  <span>{author.name}</span>
                  <wa-badge title={author.role}>{author.role_label}</wa-badge>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </header>

      {#if canListen}
        <div class="listen">
          <ActionButton onclick={listenBook} disabled={listening}>
            <wa-icon name="play"></wa-icon>
            Listen
          </ActionButton>
        </div>
      {/if}

      <dl class="facts">
        {#if book.format}
          <div class="fact"><dt>Format</dt><dd>{book.format}</dd></div>
        {/if}
        {#if book.published_date}
          <div class="fact"><dt>Published</dt><dd>{book.published_date}</dd></div>
        {/if}
        {#if book.language_name}
          <div class="fact"><dt>Language</dt><dd>{book.language_name}</dd></div>
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
                <span class="mono" title={identifier.value}>{formatIdentifier(identifier.value)}</span>
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
                <dd class="path">
                  <ActionButton
                    onclick={() => revealFile(path)}
                    disabled={file.file_status === 'missing'}
                  >
                    <wa-icon name="folder-open"></wa-icon>
                    Open
                  </ActionButton>
                </dd>
              </div>
            {/if}
          </dl>
          {#if file.file_status === 'missing'}
            <p class="error">The linked file is missing — the original was moved or deleted.</p>
            <ActionButton onclick={relinkFile} disabled={relinking}>Re-link file…</ActionButton>
          {/if}
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

  .drawer {
    --size: 50vw;
  }

  .header {
    display: flex;
    gap: var(--wa-space-l);
    align-items: center;
    flex-direction: column;
  }

  .cover {
    --size: 50%;
    width: var(--size);
    min-width: var(--size);
    height: calc(var(--size) * 1.5);
    margin: 0 auto;
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

  .detail-placeholder {
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

    > span {
      flex: 1 1;
    }
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

  .path {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--wa-space-s);
    min-width: 0;
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
