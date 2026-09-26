<script lang="ts">
import { EpubNavigator } from '@readium/navigator'
import { Locator, Manifest, Publication } from '@readium/shared'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onDestroy, onMount } from 'svelte'
import { page } from '$app/state'
import ActionButton from '../../../lib/components/ActionButton.svelte'
import { ReaderFetcher } from '../../../lib/reader/fetcher'
import { createLoadSession } from '../../../lib/reader/loadSession'
import { loadReaderPublication } from '../../../lib/reader/read'

const editionId = $derived(page.params.editionId ?? '')

const session = createLoadSession()

let container = $state<HTMLElement | undefined>(undefined)
let navigator = $state<EpubNavigator | undefined>(undefined)
let phase = $state<'loading' | 'ready' | 'error'>('loading')
let failure = $state('')
let title = $state('Reader')

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : 'Could not open this book.'
}

async function load() {
  const generation = session.begin()
  await navigator?.destroy().catch(() => {})
  navigator = undefined
  if (!session.isCurrent(generation)) return
  phase = 'loading'
  failure = ''
  let instance: EpubNavigator | undefined
  try {
    if (!editionId) throw new Error('No edition id was provided to the reader.')
    const { manifest, positions } = await loadReaderPublication(editionId)
    if (!session.isCurrent(generation)) return
    const deserialized = Manifest.deserialize(manifest)
    if (!deserialized) {
      throw new Error(
        `The publication manifest for edition ${editionId} is invalid. Try re-importing the file.`,
      )
    }
    const publication = new Publication({
      manifest: deserialized,
      fetcher: new ReaderFetcher(editionId),
    })
    title = publication.metadata.title.getTranslation() || `Reader — ${editionId}`
    const locators = (Array.isArray(positions) ? positions : [])
      .map((item) => Locator.deserialize(item))
      .filter((locator) => locator !== undefined)
    const host = container
    if (!host) throw new Error('The reader container is not available.')
    instance = new EpubNavigator(
      host,
      publication,
      {
        frameLoaded: () => {},
        positionChanged: () => {},
        timelineItemChanged: () => {},
        tap: () => false,
        click: () => false,
        zoom: () => {},
        miscPointer: () => {},
        scroll: () => {},
        customEvent: () => {},
        handleLocator: () => false,
        textSelected: () => {},
        contentProtection: () => {},
        contextMenu: () => {},
        peripheral: () => {},
      },
      locators,
    )
    await instance.load()
    if (!session.isCurrent(generation)) {
      await instance.destroy().catch(() => {})
      return
    }
    navigator = instance
    phase = 'ready'
  } catch (error) {
    if (instance) await instance.destroy().catch(() => {})
    if (!session.isCurrent(generation)) return
    navigator = undefined
    phase = 'error'
    failure = messageOf(error)
  }
}

function previous() {
  navigator?.goBackward(false, () => {})
}

function next() {
  navigator?.goForward(false, () => {})
}

async function close() {
  await getCurrentWindow().close()
}

onMount(() => {
  void load()
})

onDestroy(() => {
  session.invalidate()
  const instance = navigator
  navigator = undefined
  void instance?.destroy().catch(() => {})
})
</script>

<main class="reader">
  <header class="bar">
    <h1 class="title">{title}</h1>
    <div class="controls">
      <ActionButton onclick={previous} disabled={phase !== 'ready'}>
        <wa-icon name="chevron-left"></wa-icon>
        Previous
      </ActionButton>
      <ActionButton onclick={next} disabled={phase !== 'ready'}>
        Next
        <wa-icon name="chevron-right"></wa-icon>
      </ActionButton>
      <ActionButton onclick={() => void close()}>Close</ActionButton>
    </div>
  </header>

  {#if phase === 'loading'}
    <p class="muted">Loading…</p>
  {:else if phase === 'error'}
    <div class="error">
      <p>Could not open this book: {failure}</p>
      <ActionButton onclick={() => void load()}>Retry</ActionButton>
    </div>
  {/if}

  <div class="viewport" bind:this={container}></div>
</main>

<style>
  .reader {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-s);
    padding: var(--wa-space-s) var(--wa-space-m);
  }

  .title {
    margin: 0;
    font-size: var(--wa-font-size-m);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .controls {
    display: flex;
    gap: var(--wa-space-2xs);
  }

  .muted {
    padding: 0 var(--wa-space-m);
    color: var(--wa-color-text-secondary);
  }

  .error {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    align-items: flex-start;
    padding: 0 var(--wa-space-m);
    color: var(--wa-color-danger);
  }

  .error p {
    margin: 0;
  }

  .viewport {
    flex: 1;
    min-height: 0;
  }
</style>
