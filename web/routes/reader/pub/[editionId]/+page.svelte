<script lang="ts">
import { Locator } from '@readium/shared'
import { createHotkeys } from '@tanstack/svelte-hotkeys'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onMount } from 'svelte'
import { page } from '$app/state'
import AnnotationComposer from '$lib/components/AnnotationComposer.svelte'
import AnnotationDrawer from '$lib/components/AnnotationDrawer.svelte'
import NotesPanel from '$lib/components/NotesPanel.svelte'
import ReaderFab from '$lib/components/ReaderFab.svelte'
import ReaderFrame from '$lib/components/ReaderFrame.svelte'
import ReaderToolbar from '$lib/components/ReaderToolbar.svelte'
import { locatorWithQuote, type Quote } from '$lib/reader/anchor'
import type { Bookmark } from '$lib/reader/bookmark'
import {
  addBookmark,
  isBookmarked,
  loadBookmarks,
  locatorKey,
  removeBookmark,
  withBookmark,
  withoutBookmark,
} from '$lib/reader/bookmark'
import type { HighlightColor } from '$lib/reader/highlights'
import { updateNoteText } from '$lib/reader/notes'
import type { Annotation } from '$lib/reader/progress'
import { loadAnnotations, saveProgress } from '$lib/reader/progress'
import ReaderLayout from '../../ReaderLayout.svelte'

const editionId = $derived(page.params.editionId ?? '')

/** The navigator lifecycle lives in `ReaderFrame`; this is its imperative surface. */
type ReaderHandle = {
  previous: () => void
  next: () => void
  go: (locator: Locator) => void
  reload: () => void
  currentLocator: () => Locator | undefined
  quoteAtPosition: () => Quote | undefined
}

let frame = $state<ReaderHandle | undefined>(undefined)

/** Whether the publication is open; the frame owns the load it reports. */
let ready = $state(false)
let title = $state('Reader')
/** Formatted progress through the publication, reported by the frame. */
let pageProgress = $state('—')

// Annotations loaded from the edition's saved progress; appended on demand.
let annotations = $state<Annotation[]>([])

/**
 * Whether `annotations` reflects the stored list.
 *
 * Until it does, a save must not carry a list: the backend reads `null` as
 * "leave the column alone", but an empty array would blank every note.
 */
let annotationsLoaded = $state(false)

// Page bookmarks for this edition, loaded alongside the notes. A bookmark holds
// no text and no colour: it is only a place to come back to.
let bookmarks = $state<Bookmark[]>([])
let bookmarksLoaded = $state(false)

/** Serialized locator of the page on screen, published by the frame. */
let location = $state('')

/** Fraction of the whole book at the page on screen, published by the frame. */
let bookProgression = $state(0)

/** Whether a bookmark save is in flight, so a second click cannot duplicate it. */
let bookmarking = $state(false)

/** Whether the page on screen already carries a bookmark. */
const bookmarked = $derived(
  location !== '' && isBookmarked(bookmarks, { kind: 'epub', locator: location }),
)

/** Whether the note list is open. Highlights are always drawn regardless. */
let panelOpen = $state(false)

/** Note whose highlight the reader clicked, shown in the annotation drawer. */
let activeNote = $state<Annotation | null>(null)

/** Page the drawer opened on; a page turn dismisses it. */
let drawerPage = ''

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

function handleTextSelect(selected: { text: string; locator?: string } | null) {
  selection = selected
}

/** Open the drawer on the note a reader clicked inside the publication. */
function activateNote(id: string): boolean {
  const annotation = annotations.find((item) => item.id === id)
  if (!annotation) return false
  activeNote = annotation
  drawerPage = locatorKey(location)
  return true
}

/**
 * Dismiss the drawer once the page underneath it changes. The note belongs to
 * the page it was opened on, so a sheet left open over other text reads as the
 * wrong annotation. The page, not the raw locator: a tap republishes the
 * locator without turning the page.
 */
$effect(() => {
  if (locatorKey(location) !== drawerPage) activeNote = null
})

/** Turn back one page, through the frame's navigator. */
function previous() {
  frame?.previous()
}

/** Turn forward one page, through the frame's navigator. */
function next() {
  frame?.next()
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
  frame?.go(locator)
  panelOpen = false
  activeNote = null
}

/**
 * Write the note list through immediately, without the reading position.
 *
 * The frame owns the position and persists it on its own schedule, so this
 * leaves that column alone. Persisting synchronously (not through a debounce)
 * keeps a quick close from resurrecting a note the reader just removed.
 */
function persistNotes() {
  void saveProgress(editionId, { kind: 'epub', annotations })
}

/**
 * Bookmark the page on screen.
 *
 * The frame publishes the same locator a note anchors to, so the bookmark lands
 * on the page the reader is looking at rather than on a stored progression. The
 * recorded number is the page's fraction of the whole book — the locator's own
 * `progression` is relative to its chapter and would read as a book figure.
 * Bookmarking only ever adds: a page that already carries a bookmark has its
 * button disabled, and the panel owns removal.
 */
async function bookmarkCurrentPage() {
  if (!bookmarksLoaded || bookmarking || location === '') return
  if (isBookmarked(bookmarks, { kind: 'epub', locator: location })) return
  bookmarking = true
  try {
    const added = await addBookmark(editionId, {
      kind: 'epub',
      locator: location,
      progress: bookProgression,
    })
    bookmarks = withBookmark(bookmarks, added)
  } finally {
    bookmarking = false
  }
}

/** Move the reader to a bookmark and dismiss the list so the page shows. */
function visitBookmark(bookmark: Bookmark) {
  if (!bookmark.locator) return
  let locator: Locator | undefined
  try {
    locator = Locator.deserialize(JSON.parse(bookmark.locator))
  } catch {
    return
  }
  if (!locator) return
  frame?.go(locator)
  panelOpen = false
  activeNote = null
}

/** Drop a bookmark without moving the reader. */
async function removeBookmarkEntry(bookmark: Bookmark) {
  if (!(await removeBookmark(bookmark.id))) return
  bookmarks = withoutBookmark(bookmarks, bookmark.id)
  if (bookmarks.length === 0 && annotations.length === 0) panelOpen = false
}

function saveAnnotation(text: string, color?: HighlightColor) {
  if (editingId) {
    // Keep the anchor and creation time; only the reader's words and colour change.
    annotations = updateNoteText(annotations, editingId, text, color)
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
 * Merge a text anchor into the current reading position's locator.
 *
 * Without one, a note carries only a progression, which the decorator cannot
 * resolve — so it could never be highlighted. The quote (plus its surrounding
 * context) pins the decoration to the text the reader picked while keeping the
 * position itself intact.
 */
function locatorForNote(quote: Quote | undefined): string | undefined {
  const current = frame?.currentLocator()
  if (!current) return undefined
  return locatorWithQuote(JSON.stringify(current.serialize()), quote)
}

/** Open the dialog, capturing the anchor now. */
function openComposer() {
  pendingQuote = frame?.quoteAtPosition()
  editingId = null
  composerOpen = true
}

function closeComposer() {
  composerOpen = false
  pendingQuote = undefined
  editingId = null
}

/** Load the stored note list once the publication is open. */
$effect(() => {
  if (!ready || annotationsLoaded) return
  void (async () => {
    annotations = await loadAnnotations(editionId)
    annotationsLoaded = true
  })()
})

/** Load the stored bookmarks once the publication is open. */
$effect(() => {
  if (!ready || bookmarksLoaded) return
  void (async () => {
    bookmarks = await loadBookmarks(editionId)
    bookmarksLoaded = true
  })()
})

createHotkeys([
  {
    hotkey: 'A',
    callback: () => {
      if (ready && annotationsLoaded && selection !== null && !composerOpen) openComposer()
    },
  },
  {
    hotkey: 'ArrowLeft',
    callback: () => {
      if (ready) previous()
    },
  },
  {
    hotkey: 'ArrowRight',
    callback: () => {
      if (ready) next()
    },
  },
  {
    hotkey: 'J',
    callback: () => {
      if (ready) previous()
    },
  },
  {
    hotkey: 'K',
    callback: () => {
      if (ready) next()
    },
  },
  {
    hotkey: 'H',
    callback: () => {
      if (ready) previous()
    },
  },
  {
    hotkey: 'L',
    callback: () => {
      if (ready) next()
    },
  },
])

async function close() {
  await getCurrentWindow().close()
}

onMount(() => {
  function onKey(e: KeyboardEvent) {
    if (e.key === 'a' && ready && annotationsLoaded && selection !== null && !composerOpen) {
      openComposer()
    }
    if (e.key === 'ArrowLeft' && ready) previous()
    if (e.key === 'ArrowRight' && ready) next()
    if (!composerOpen && (e.key === 'j' || e.key === 'J')) previous()
    if (!composerOpen && (e.key === 'k' || e.key === 'K')) next()
    if (!composerOpen && (e.key === 'h' || e.key === 'H')) previous()
    if (!composerOpen && (e.key === 'l' || e.key === 'L')) next()
  }
  document.addEventListener('keydown', onKey)
  return () => document.removeEventListener('keydown', onKey)
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
  if (activeNote?.id === note.id) activeNote = null

  persistNotes()

  // Nothing left to list: close it. The frame drops the highlight with the note.
  if (annotations.length === 0) panelOpen = false
}
</script>

<ReaderLayout>
  <ReaderToolbar
    {title}
    {ready}
    canAnnotate={annotationsLoaded}
    {bookmarked}
    canBookmark={bookmarksLoaded}
    onprevious={previous}
    onnext={next}
    onannotate={openComposer}
    onbookmark={() => void bookmarkCurrentPage()}
    onclose={() => void close()}
  />

  {#if composerOpen}
    <AnnotationComposer
      quote={pendingQuote?.highlight ?? selection?.text}
      placeholder="Note text"
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
    <ReaderFrame
      bind:this={frame}
      bind:ready
      bind:title
      bind:progress={pageProgress}
      {editionId}
      {annotations}
      bind:location
      bind:progression={bookProgression}
      keyboard={ready && !composerOpen}
      ontextselect={handleTextSelect}
      onannotationactivate={activateNote}
    />

    {#if (annotations.length > 0 || bookmarks.length > 0) && ready}
      <ReaderFab
        progress={pageProgress}
        count={annotations.length}
        bookmarks={bookmarks.length}
        open={panelOpen}
        expanded={panelOpen || activeNote !== null}
        ontoggle={togglePanel}
      />
    {/if}

    {#if panelOpen && (annotations.length > 0 || bookmarks.length > 0)}
      <NotesPanel
        notes={annotations}
        {bookmarks}
        onvisit={jumpTo}
        onedit={editNote}
        ondelete={deleteNote}
        onrecolor={recolorNote}
        onvisitbookmark={visitBookmark}
        onremovebookmark={(bookmark) => void removeBookmarkEntry(bookmark)}
        onclose={togglePanel}
      />
    {/if}

    {#if activeNote}
      <AnnotationDrawer annotation={activeNote} onclose={() => (activeNote = null)} />
    {/if}
  </div>
</ReaderLayout>

<style>
  /*
    The pane is the positioning context for everything the reader layers over
    the text: the frame, the reader bar, the note list, and the annotation
    drawer. None of them takes layout space, so the book never reflows under
    them.
  */
  .stage {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  :global(iframe.readium-navigator-iframe) {
    height: 100%;
  }
</style>
