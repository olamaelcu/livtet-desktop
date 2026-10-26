<script lang="ts">
import { EpubNavigator } from '@readium/navigator'
import { Locator, Manifest, Publication } from '@readium/shared'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onDestroy, onMount } from 'svelte'
import { page } from '$app/state'
import ActionButton from '$lib/components/ActionButton.svelte'
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import NotesPanel from '$lib/components/NotesPanel.svelte'
import { type Quote, readingPositionQuote, selectedQuote } from '$lib/reader/anchor'
import { buildAnnotationDecorations } from '$lib/reader/decorations'
import { ReaderFetcher } from '$lib/reader/fetcher'
import type { HighlightColor } from '$lib/reader/highlights'
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

/**
 * Whether `annotations` reflects the stored list.
 *
 * Until it does, a save must not carry a list: the backend reads `null` as
 * "leave the column alone", but an empty array would blank every note.
 */
let annotationsLoaded = $state(false)

/** Whether the note list is open. Highlights are always drawn regardless. */
let panelOpen = $state(false)

/** Note card floating over the clicked highlight, in stage coordinates. */
let activeNote = $state<{ annotation: Annotation; top: number; left: number } | null>(null)

// Text currently selected in the publication, captured by `textSelected`.
let selection = $state<{ text: string; locator?: string } | null>(null)
let composerOpen = $state(false)

/**
 * Anchor captured when the dialog opens.
 *
 * The dialog is modal, so the page cannot move underneath it: one capture
 * serves both the quoted preview and the saved locator.
 */
let pendingQuote = $state<Quote | undefined>(undefined)

/** Note being edited by the dialog; `null` while writing a new one. */
let editingId = $state<string | null>(null)

// Track the latest locator received from positionChanged
let latestLoc: Locator | undefined

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : 'Could not open this book.'
}

// Debounced save that uses the latest captured locator
const saveEpubProgress = debouncedSave(
  () => editionId,
  () => {
    const owned = annotationsLoaded ? annotations : undefined
    if (!latestLoc) return { kind: 'epub' as const, annotations: owned }
    return {
      kind: 'epub' as const,
      epub: { locator: JSON.stringify(latestLoc) },
      annotations: owned,
    }
  },
  250,
)

/**
 * Push every anchored annotation into the navigator as a highlight.
 *
 * Highlights are not tied to the note list: annotated text stays marked while
 * the reader reads, and the FAB only opens the list.
 */
function syncDecorations() {
  navigator?.applyDecorations(buildAnnotationDecorations(annotations), ANNOTATION_GROUP)
}

/** Open or close the note list. Highlights are unaffected. */
function togglePanel() {
  panelOpen = !panelOpen
  if (!panelOpen) activeNote = null
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
    annotations = progress?.annotations ?? []
    annotationsLoaded = true
    syncDecorations()
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
/** Write the current note list through immediately, with the reading position. */
function persistNotes() {
  void saveProgress(editionId, {
    kind: 'epub',
    epub: latestLoc ? { locator: JSON.stringify(latestLoc) } : undefined,
    annotations,
  })
  // Highlights follow the list: new tint, edited text, or a removal.
  syncDecorations()
}

function saveAnnotation(text: string, color?: HighlightColor) {
  if (editingId) {
    // Keep the anchor and creation time; only the reader's words and colour change.
    annotations = annotations.map((item) =>
      item.id === editingId ? { ...item, text, color: color ?? item.color } : item,
    )
    editingId = null
    closeComposer()
    persistNotes()
    return
  }

  const annotation: Annotation = {
    id: crypto.randomUUID(),
    text,
    quote: pendingQuote?.highlight ?? selection?.text,
    locator: locatorForNote(pendingQuote),
    color,
    createdAt: new Date().toISOString(),
  }
  annotations = [...annotations, annotation]
  selection = null
  closeComposer()
  persistNotes()
}

/** Change one note's highlight colour without opening the dialog. */
function recolorNote(note: Annotation, color: HighlightColor) {
  if (note.color === color) return
  annotations = annotations.map((item) => (item.id === note.id ? { ...item, color } : item))
  persistNotes()
}

/** Open the dialog on an existing note, pre-filled. */
function editNote(note: Annotation) {
  pendingQuote = undefined
  editingId = note.id
  composerOpen = true
}

/**
 * Merge a text anchor into the reading position's locator.
 *
 * Without one, a note carries only a progression, which the decorator cannot
 * resolve — so it could never be highlighted. The quote (plus its surrounding
 * context) pins the decoration to the text the reader picked while keeping the
 * position itself intact.
 */
function locatorForNote(quote: Quote | undefined): string | undefined {
  if (!latestLoc) return undefined
  const raw = latestLoc.serialize() as { text?: Record<string, unknown> }
  if (!quote) return JSON.stringify(raw)
  return JSON.stringify({ ...raw, text: { ...raw.text, ...quote } })
}

/**
 * Open the dialog, capturing the anchor now.
 *
 * The selection is read from the page's DOM rather than the `textSelected`
 * event: the DOM range carries the surrounding context that pins a repeated
 * phrase to the occurrence the reader actually picked.
 */
function openComposer() {
  pendingQuote = container
    ? (selectedQuote(container) ?? readingPositionQuote(container))
    : undefined
  editingId = null
  composerOpen = true
}

function closeComposer() {
  composerOpen = false
  pendingQuote = undefined
  editingId = null
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
      annotations: annotationsLoaded ? annotations : undefined,
    })
  }
  latestLoc = undefined
  void instance?.destroy().catch(() => {})
})

/**
 * Remove a note, persist the remaining list immediately, and drop its highlight.
 *
 * There is no undo and WebKitGTK implements no `window.confirm`, so the list
 * carries an explicit Delete button per note rather than a confirmation dialog.
 * Writing synchronously (not through the debounce) keeps a quick close from
 * resurrecting the note.
 */
function deleteNote(note: Annotation) {
  annotations = annotations.filter((item) => item.id !== note.id)
  if (activeNote?.annotation.id === note.id) activeNote = null

  void saveProgress(editionId, {
    kind: 'epub',
    epub: latestLoc ? { locator: JSON.stringify(latestLoc) } : undefined,
    annotations,
  })

  // Nothing left to list: close it. `syncDecorations` drops the highlight.
  if (annotations.length === 0) panelOpen = false
  syncDecorations()
}
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
      <ActionButton onclick={openComposer} disabled={phase !== 'ready' || !annotationsLoaded}>
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
      quote={pendingQuote?.highlight ?? selection?.text}
      showColors
      initialText={editingId ? annotations.find((item) => item.id === editingId)?.text : undefined}
      initialColor={editingId
        ? annotations.find((item) => item.id === editingId)?.color
        : undefined}
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
          onclick={togglePanel}
          aria-pressed={panelOpen}
          aria-haspopup="dialog"
          aria-expanded={panelOpen || activeNote !== null}
        >
          <wa-icon name="comment-dots"></wa-icon>
          {annotations.length} Note{#if annotations.length != 1}s{/if}
        </ActionButton>
      </div>
    {/if}

    {#if panelOpen && annotations.length > 0}
      <NotesPanel
        notes={annotations}
        onvisit={jumpTo}
        onedit={editNote}
        ondelete={deleteNote}
        onrecolor={recolorNote}
        onclose={togglePanel}
      />
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
    background: var(--wa-color-surface-default);
    box-shadow: var(--wa-shadow-m, 0 4px 12px rgb(0 0 0 / 0.18));
    transform: translate(0, 0.5rem);
  }

  .note-quote {
    display: block;
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
    border-inline-start: 2px solid var(--wa-color-border);
    color: var(--wa-color-text-secondary);
    font-style: italic;
  }

  .note-text {
    margin: 0 0 var(--wa-space-2xs);
    padding: var(--wa-space-2xs);
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
