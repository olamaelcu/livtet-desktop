<script lang="ts">
import { EpubNavigator } from '@readium/navigator'
import { Locator, Manifest, Publication } from '@readium/shared'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onDestroy, onMount } from 'svelte'
import { page } from '$app/state'
import ActionButton from '$lib/components/ActionButton.svelte'
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import { buildAnnotationDecorations } from '$lib/reader/decorations'
import { ReaderFetcher } from '$lib/reader/fetcher'
import { createLoadSession } from '$lib/reader/loadSession'
import type { Annotation } from '$lib/reader/progress'
import { debouncedSave, loadProgress, saveProgress } from '$lib/reader/progress'
import { loadEpubReaderPublication } from '$lib/reader/read'
import ReaderLayout from '../../ReaderLayout.svelte'

const editionId = $derived(page.params.editionId ?? '')

const session = createLoadSession()

/** Decoration group owning every reader annotation highlight. */
const ANNOTATION_GROUP = 'reader-annotations'

let container = $state<HTMLElement | undefined>(undefined)
let navigator = $state<EpubNavigator | undefined>(undefined)
let phase = $state<'loading' | 'ready' | 'error'>('loading')
let failure = $state('')
let title = $state('Reader')

// Annotations loaded from the edition's saved progress; appended on demand.
let annotations = $state<Annotation[]>([])

/** Whether the annotation layer (highlights) is revealed by the FAB. */
let notesVisible = $state(false)

/** Whether the note list is open. A jump closes it but keeps the highlights. */
let panelOpen = $state(false)

/** Note card floating over the clicked highlight, in stage coordinates. */
let activeNote = $state<{ annotation: Annotation; top: number; left: number } | null>(null)

// Text currently selected in the publication, captured by `textSelected`.
let selection = $state<{ text: string; locator?: string } | null>(null)
let composerOpen = $state(false)

// Track the latest locator received from positionChanged
let latestLoc: Locator | undefined

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : 'Could not open this book.'
}

// Debounced save that uses the latest captured locator
const saveEpubProgress = debouncedSave(
  () => editionId,
  () => {
    if (!latestLoc) return { kind: 'epub' as const, annotations }
    return {
      kind: 'epub' as const,
      epub: { locator: JSON.stringify(latestLoc) },
      annotations,
    }
  },
  250,
)

/** Push the current annotation highlights into the navigator, or clear them. */
function syncDecorations() {
  navigator?.applyDecorations(
    notesVisible ? buildAnnotationDecorations(annotations) : [],
    ANNOTATION_GROUP,
  )
}

/**
 * Reveal or hide the annotation layer, with its note list.
 *
 * A jump hides the list but keeps the highlights; while those are still on,
 * the FAB reopens the list instead of tearing the layer down.
 */
function toggleNotes() {
  if (notesVisible && !panelOpen) {
    panelOpen = true
    return
  }
  notesVisible = !notesVisible
  panelOpen = notesVisible
  if (!notesVisible) activeNote = null
  syncDecorations()
}

/**
 * Move the reader to an annotation's saved locator and dismiss the list so the
 * text it points at is not hidden behind it. Highlights stay revealed.
 */
function jumpTo(note: Annotation) {
  if (!note.locator) return
  let locator: Locator | undefined
  try {
    locator = Locator.deserialize(JSON.parse(note.locator))
  } catch {
    return
  }
  if (!locator) return
  navigator?.go(locator, false, () => {})
  panelOpen = false
  activeNote = null
}

async function load() {
  const generation = session.begin()
  await navigator?.destroy().catch(() => {})
  navigator = undefined
  latestLoc = undefined
  if (!session.isCurrent(generation)) return
  phase = 'loading'
  failure = ''
  let instance: EpubNavigator | undefined
  try {
    if (!editionId) throw new Error('No edition id was provided to the reader.')
    const { manifest, positions } = await loadEpubReaderPublication(editionId)
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
        positionChanged: (locator: Locator) => {
          latestLoc = locator
          // A selection belongs to the frame it was made in; drop it once the
          // reader moves so a later note never quotes stale text.
          selection = null
          saveEpubProgress()
        },
        timelineItemChanged: () => {},
        tap: () => false,
        click: () => false,
        zoom: () => {},
        miscPointer: () => {},
        scroll: () => {},
        customEvent: () => {},
        handleLocator: () => false,
        textSelected: (selected: { text: string; locator?: Locator }) => {
          selection = {
            text: selected.text,
            locator: selected.locator ? JSON.stringify(selected.locator) : undefined,
          }
        },
        contentProtection: () => {},
        contextMenu: () => {},
        peripheral: () => {},
      },
      locators,
    )

    // Clicking a highlight floats its note over the text at the highlight rect.
    instance.registerDecorationObserver(ANNOTATION_GROUP, {
      onDecorationActivated: (event) => {
        const annotation = annotations.find((item) => item.id === event.decoration.id)
        if (!annotation) return false
        activeNote = { annotation, top: event.rect.top, left: event.rect.left }
        return true
      },
    })

    await instance.load()
    if (!session.isCurrent(generation)) {
      await instance.destroy().catch(() => {})
      return
    }
    navigator = instance
    phase = 'ready'

    // Load saved progress and resume to last position
    const progress = await loadProgress(editionId)
    if (progress?.annotations) annotations = progress.annotations
    if (progress?.epub?.locator) {
      try {
        const locator = Locator.deserialize(JSON.parse(progress.epub.locator))
        // `go(locator, animated, cb)` — the middle argument is a boolean, not
        // the completion callback, so both trailing arguments are required.
        if (locator) navigator.go(locator, false, () => {})
      } catch {
        // If parsing fails, just use the initial positions
      }
    }
  } catch (error) {
    if (instance) await instance.destroy().catch(() => {})
    if (!session.isCurrent(generation)) return
    navigator = undefined
    phase = 'error'
    failure = messageOf(error)
  }
}

/**
 * Record the composer's note text against the current selection, falling back
 * to the viewport locator when nothing is selected.
 *
 * The note is persisted immediately so it survives a window close without
 * waiting for the debounced position save. WebKitGTK implements no
 * `window.prompt`, so the text is collected in an in-page composer instead.
 */
function saveAnnotation(text: string) {
  const position = latestLoc ? JSON.stringify(latestLoc) : undefined
  const annotation: Annotation = {
    id: crypto.randomUUID(),
    text,
    quote: selection?.text,
    locator: selection?.locator ?? position,
  }
  annotations = [...annotations, annotation]
  void saveProgress(editionId, {
    kind: 'epub',
    epub: position ? { locator: position } : undefined,
    annotations,
  })
  selection = null
  closeComposer()
  // A note written mid-read is already anchored: reveal its highlight.
  notesVisible = true
  syncDecorations()
}

/** Open the composer, keeping any selection captured by `textSelected`. */
function openComposer() {
  composerOpen = true
}

function closeComposer() {
  composerOpen = false
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
  // Save final progress before resetting latestLoc
  if (instance && latestLoc) {
    void saveProgress(editionId, {
      kind: 'epub',
      epub: { locator: JSON.stringify(latestLoc) },
      annotations,
    })
  }
  latestLoc = undefined
  void instance?.destroy().catch(() => {})
})
</script>

<ReaderLayout>
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
      <ActionButton onclick={openComposer} disabled={phase !== 'ready'}>
        <wa-icon name="comment"></wa-icon>
        Annotate
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

  {#if composerOpen}
    <AnnotationComposer
      placeholder="Note text"
      quote={selection?.text}
      onsave={saveAnnotation}
      oncancel={closeComposer}
    />
  {/if}

  <div class="stage">
    <!-- Pane grows this; the Readium iframe scrolls inside it. -->
    <div class="viewport" data-scroll-region bind:this={container}></div>

    {#if annotations.length > 0 && phase === 'ready'}
      <div class="fab">
        <ActionButton
          variant="brand"
          onclick={toggleNotes}
          aria-pressed={notesVisible}
          aria-haspopup="dialog"
          aria-expanded={panelOpen || activeNote !== null}
        >
          <wa-icon name="comment-dots"></wa-icon>
          {annotations.length} Note{#if annotations.length != 1}s{/if}
        </ActionButton>
      </div>
    {/if}

    {#if notesVisible && panelOpen && annotations.length > 0}
      <div class="panel" role="dialog" aria-label="Notes">
        <p class="panel-heading">Notes ({annotations.length})</p>
        {#each annotations as note (note.id)}
          <button type="button" class="panel-note" onclick={() => jumpTo(note)}>
            {#if note.quote}
              <span class="note-quote">{note.quote}</span>
            {/if}
            <span class="note-text">{note.text}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if activeNote}
      <div
        class="note"
        role="dialog"
        aria-label="Annotation"
        style:top="{activeNote.top}px"
        style:left="{activeNote.left}px"
      >
        {#if activeNote.annotation.quote}
          <blockquote class="note-quote">{activeNote.annotation.quote}</blockquote>
        {/if}
        <p class="note-text">{activeNote.annotation.text}</p>
        <div class="note-actions">
          <ActionButton size="small" onclick={() => (activeNote = null)}>Close</ActionButton>
        </div>
      </div>
    {/if}
  </div>
</ReaderLayout>

<style>
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

  /* Positioning context for the FAB and the floating note cards. */
  .stage {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .fab {
    position: absolute;
    inset-block-end: var(--wa-space-m);
    inset-inline-end: var(--wa-space-m);
    z-index: 2;
  }

  /*
    The card floats over the text and never takes layout space. It is anchored
    at the highlight rect reported by the navigator, clamped so it cannot leave
    the reading pane.
  */
  .note {
    position: absolute;
    z-index: 2;
    max-width: 20rem;
    max-height: 16rem;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-border-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-border);
    background: var(--wa-color-surface);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
    transform: translate(0, 0.5rem);
  }

  /* Floating list of notes; a click jumps to where the note was written. */
  .panel {
    position: absolute;
    z-index: 2;
    inset-block-end: var(--wa-space-m);
    inset-inline-start: var(--wa-space-m);
    width: min(18rem, calc(100% - 2 * var(--wa-space-m)));
    max-height: 40%;
    overflow: auto;
    padding: var(--wa-space-s);
    border-radius: var(--wa-border-radius-m, 0.5rem);
    border: 1px solid var(--wa-color-border);
    background: var(--wa-color-surface);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
  }

  .panel-heading {
    margin: 0 0 var(--wa-space-2xs);
    font-size: var(--wa-font-size-xs, 0.8125rem);
    color: var(--wa-color-text-secondary);
  }

  .panel-note {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
    width: 100%;
    padding: var(--wa-space-2xs) 0;
    border: 0;
    border-block-start: 1px solid var(--wa-color-border);
    background: none;
    color: inherit;
    font: inherit;
    font-size: var(--wa-font-size-s);
    text-align: start;
    cursor: pointer;
  }

  .panel-note:hover,
  .panel-note:focus-visible {
    background: var(--wa-color-surface-lowered, rgb(0 0 0 / 0.04));
  }

  .note-quote {
    display: block;
    margin: 0 0 var(--wa-space-2xs);
    padding-inline-start: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .note-text {
    margin: 0 0 var(--wa-space-2xs);
  }

  .note-actions {
    display: flex;
    justify-content: flex-end;
  }

  .viewport {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    width: 100%!important;
    border-top: 2rem solid #fff;
    border-bottom: 2rem solid #fff;
    background-color: #fff;

    :global(iframe.readium-navigator-iframe) {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      max-width: 100%;
      max-height: 100%;
      margin: 0;
      border: 0;
    }
  }
</style>
