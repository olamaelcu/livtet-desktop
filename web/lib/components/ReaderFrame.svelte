<script lang="ts">
import { EpubNavigator, EpubPreferences } from '@readium/navigator'
import { Locator, Manifest, Publication } from '@readium/shared'
import { onDestroy, onMount } from 'svelte'
import ActionButton from '$lib/components/ActionButton.svelte'
import { type Quote, readingPositionQuote, selectedQuote } from '$lib/reader/anchor'
import {
  ANNOTATION_GROUP,
  buildAnnotationDecorations,
  buildSemanticDecorations,
  SEMANTIC_FAMILIES,
  SEMANTIC_GROUP,
  SEMANTIC_STYLESHEET,
} from '$lib/reader/decorations'
import { ReaderFetcher } from '$lib/reader/fetcher'
import { DEFAULT_FONT_SIZE, stepFontSize } from '$lib/reader/fontSize'
import { createLoadSession } from '$lib/reader/loadSession'
import type { Annotation } from '$lib/reader/progress'
import { debouncedSave, loadProgress, saveProgress } from '$lib/reader/progress'
import { loadEpubReaderPublication } from '$lib/reader/read'

type Phase = 'loading' | 'ready' | 'error'

interface Props {
  /** Edition the publication belongs to; drives the fetcher and the manifest lookup. */
  editionId: string
  /** Notes to draw as highlights inside the publication. */
  annotations: Annotation[]
  /** False while a dialog has focus: frame page-turn keys stay inert. */
  keyboard: boolean
  /** Whether the publication is open and can be navigated. */
  ready: boolean
  title: string
  /** Percentage through the publication, for the note button. */
  progress: string
  /** Serialized locator of the page on screen; empty until the navigator reports one. */
  location: string
  /**
   * Fraction of the whole publication at the page on screen (0-1), for a
   * bookmark's recorded position. The locator's own `progression` is relative
   * to its resource, so it cannot stand in for this.
   */
  progression: number
  ontextselect: (selection: { text: string; locator?: string } | null) => void
  /** Resolve a clicked highlight; `false` swallows the click. */
  onannotationactivate: (id: string) => boolean
}

let {
  editionId,
  annotations,
  keyboard,
  ready = $bindable(false),
  title = $bindable('Reader'),
  progress = $bindable('—'),
  location = $bindable(''),
  progression = $bindable(0),
  ontextselect,
  onannotationactivate,
}: Props = $props()

const session = createLoadSession()

let container = $state<HTMLElement | undefined>(undefined)
let navigator = $state<EpubNavigator | undefined>(undefined)
/** Whether the navigator has finished loading and bound its frame channels. */
let settled = $state(false)
/** Kept hidden until the saved position is applied, so the cover never flashes first. */
let revealed = $state(false)
let frameWindow = $state<Window | undefined>(undefined)
let frameKeyHandler: ((e: KeyboardEvent) => void) | undefined

/** Phase of this component's own load; the page only needs {@link ready}. */
let phase = $state<Phase>('loading')
let failure = $state('')
/** Latest locator the navigator reported, and what progress saves carry. */
let latestLoc = $state<Locator | null>(null)

// Position only: the note list is the page's state and it persists its own writes.
const savePosition = debouncedSave(
  () => editionId,
  () => ({
    kind: 'epub' as const,
    epub: latestLoc ? { locator: JSON.stringify(latestLoc) } : undefined,
  }),
  250,
)

const computedStyle = () => getComputedStyle(document.documentElement)

/**
 * Presentation the reader owns rather than the publication: the window's colours and the
 * text size the reader asked for.
 *
 * This one object is both Readium's defaults (the first frame the navigator opens paints
 * with it) and the preferences submitted whenever it changes, so colours and text size
 * travel the same path and a frame opened later starts on the reader's settings.
 */
const presentation = $state({
  /** Multiplier on the frame's zoom: 1 is 100%, Readium accepts 0.7–4. */
  fontSize: DEFAULT_FONT_SIZE,
  // SSR renders without a DOM. These are placeholders only for that pass:
  // extractRootVariables() re-reads the live values as soon as the reader
  // mounts, and Readium never sees a frame before that.
  ...(typeof document === 'undefined'
    ? {
        backgroundColor: 'transparent',
        textColor: 'inherit',
        fontFamily: 'inherit',
        linkColor: 'inherit',
        visitedColor: 'inherit',
      }
    : {
        backgroundColor: computedStyle().backgroundColor,
        textColor: computedStyle().color,
        fontFamily: computedStyle().fontFamily,
        linkColor: getComputedStyle(document.querySelector('a') || document.documentElement).color,
        visitedColor: getComputedStyle(
          document.querySelector('a:visited') || document.documentElement,
        ).color,
      }),
})

function extractRootVariables() {
  const isDark = window.matchMedia
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : false
  const cs = computedStyle()
  presentation.backgroundColor = cs.backgroundColor
  presentation.textColor = cs.color
  presentation.fontFamily = cs.fontFamily

  presentation.linkColor = getComputedStyle(
    document.querySelector('a') || document.documentElement,
  ).color
  presentation.visitedColor = getComputedStyle(
    document.querySelector('a:visited') || document.documentElement,
  ).color
  console.log(
    'isDark',
    isDark,
    presentation.backgroundColor,
    presentation.textColor,
    presentation.fontFamily,
    presentation.linkColor,
    presentation.visitedColor,
  )
}

$effect(() => {
  extractRootVariables()
  const mq = window.matchMedia ? window.matchMedia('(prefers-color-scheme: dark)') : null
  if (!mq) return
  mq.addEventListener('change', extractRootVariables)
  return () => mq.removeEventListener('change', extractRootVariables)
})

/** What the navigator was last told to paint with, so a no-op change costs no repaint. */
let submittedPresentation = ''
/** Whether a preference submit is in flight; submits are serialised. */
let applyingPresentation = false

/**
 * Push the reader's presentation into the navigator.
 *
 * Readium compiles preferences into the `--USER__*` properties of every frame, which is also
 * how a frame the navigator opens later starts on the reader's colours and text size. Each
 * submit carries the presentation as it stands when that submit begins, so a burst of
 * text-size key presses coalesces into the last value instead of interleaving two relayouts
 * (which drops a step).
 */
$effect(() => {
  const wanted = JSON.stringify(presentation)
  if (!navigator || !settled || wanted === submittedPresentation) return
  void submitPresentation()
})

async function submitPresentation() {
  if (applyingPresentation) return
  applyingPresentation = true
  try {
    while (true) {
      const instance = navigator
      if (!instance) return
      const next = { ...presentation }
      const wanted = JSON.stringify(next)
      if (wanted === submittedPresentation) return
      // A real `EpubPreferences`: `submitPreferences` merges by copying the argument's own
      // fields, so an object that only carries a `merging` method would submit nothing.
      await instance.submitPreferences(
        new EpubPreferences({
          backgroundColor: next.backgroundColor,
          textColor: next.textColor,
          fontFamily: next.fontFamily,
          fontSize: next.fontSize,
          linkColor: next.linkColor,
          visitedColor: next.visitedColor,
          // Ligatures are the reader's choice too, not the publication's; every submit
          // carries them so nothing falls back to the publication's typography.
          ligatures: true,
        }),
      )
      submittedPresentation = wanted
    }
  } finally {
    applyingPresentation = false
  }
}

/**
 * Push every anchored annotation into the navigator as a highlight.
 *
 * Highlights are not tied to the note list: annotated text stays marked while
 * the reader reads, and the note button only opens the list.
 */
$effect(() => {
  // Highlights travel to the frame over its comms channel, so they can only be
  // applied once `load()` has bound it; sending earlier drops the message.
  if (!navigator || !settled) return
  navigator.applyDecorations(buildAnnotationDecorations(annotations), ANNOTATION_GROUP)
})

/**
 * Tint the publication's semantic elements (`epub:type="warning"`, `note`, …).
 *
 * Same channel and timing as the notes: decorations reach the frame only once
 * `load()` bound its comms channel. The navigator drops a decoration whose
 * locator href is not the frame's resource, so every reading-order resource
 * gets its own.
 */
$effect(() => {
  if (!navigator || !settled) return
  const hrefs = (navigator.publication?.readingOrder?.items ?? []).map((item) => item.href)
  navigator.applyDecorations(buildSemanticDecorations(SEMANTIC_FAMILIES, hrefs), SEMANTIC_GROUP)
})

// Live page progress, derived from the navigator's locator and timeline.
//
// The same poll publishes the page as a serialized locator: `positionChanged`
// does not fire for every programmatic page turn, so the page cannot rely on it
// to know which page is on screen (and therefore whether it is bookmarked).
$effect(() => {
  const compute = () => {
    const loc = navigator?.currentLocator
    if (!loc) {
      progress = '—'
      location = ''
      progression = 0
      return
    }
    location = JSON.stringify(loc)
    const pos = loc.locations?.position
    const total =
      Math.max(0, ...(navigator?.timeline?.items ?? []).map((i) => i.position ?? 0)) ||
      navigator?.publication?.readingOrder?.items?.length ||
      (navigator?.timeline?.items ?? []).length ||
      1
    const resourceProg = loc.locations?.progression ?? 0
    const currentPos = (pos ?? 1) - 1 + resourceProg
    const fraction = total > 0 ? Math.min(Math.max(currentPos / total, 0), 1) : 0
    progression = fraction
    progress = total > 0 ? `${(fraction * 100).toFixed(2)}%` : '—'
  }
  compute()
  const id = setInterval(compute, 100)
  return () => clearInterval(id)
})

const NAV_ACK_TIMEOUT = 700

/** A stored locator, or `undefined` when there is none (or it is malformed). */
function parseStoredLocator(saved: string | null | undefined): Locator | undefined {
  if (!saved) return undefined
  try {
    return Locator.deserialize(JSON.parse(saved))
  } catch {
    return undefined
  }
}

/**
 * Move the reader to a locator.
 *
 * `go` swaps in the target resource's frame before locating inside it, so the command
 * can land on a channel that was never bound. A dropped locate also leaves Readium's
 * navigation latch set, which would freeze every later page turn.
 */
export function go(locator: Locator) {
  void locate(locator)
}

async function locate(locator: Locator, attempt = 0): Promise<void> {
  const target = navigator as unknown as NavigatorInternals | undefined
  if (!target) return
  const wanted = locator.locations?.position
  // A spine `position` is shared by every page of a reflowable chapter, so it
  // cannot prove arrival on its own: a bookmark or note inside the current
  // chapter would otherwise "arrive" without the reader moving at all.
  const wantedProgression = locator.locations?.progression
  const wantedHref = locator.href.split('#')[0]
  const arrived = () => {
    const now = target.currentLocator
    if (!now) return false
    if (now.href.split('#')[0] !== wantedHref) return false
    if (wanted != null && now.locations?.position !== wanted) return false
    if (wantedProgression != null) {
      const nowProgression = now.locations?.progression
      return nowProgression != null && Math.abs(nowProgression - wantedProgression) <= 0.01
    }
    return true
  }
  target.go(locator, false, () => {})
  // Readium acknowledges `go` against whichever frame is current, so an ack is not
  // proof of arrival: wait for the locator to actually match, then retry once.
  for (let waited = 0; waited < NAV_ACK_TIMEOUT; waited += 100) {
    await new Promise((resolve) => setTimeout(resolve, 100))
    if (arrived()) return
  }
  console.debug('locate did not arrive; re-syncing frame comms and retrying')
  target._isNavigating = false
  await syncFrameComms()
  if (attempt < 2) await locate(locator, attempt + 1)
}

/** Reload the publication, e.g. after a failed open. */
export function reload() {
  void load()
}

/** Where the reader currently is, for anchoring a new note. */
export function currentLocator(): Locator | undefined {
  return navigator?.currentLocator ?? latestLoc ?? undefined
}

/**
 * The text anchor at the reading position, or over the current selection.
 *
 * Read from the live DOM rather than the `textSelected` event: the DOM range
 * carries the surrounding context that pins a repeated phrase to the
 * occurrence the reader actually picked.
 */
export function quoteAtPosition(): Quote | undefined {
  const host = container
  if (!host) return undefined
  return selectedQuote(host) ?? readingPositionQuote(host)
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : 'Could not open this book.'
}

async function load() {
  const generation = session.begin()
  await navigator?.destroy().catch(() => {})
  navigator = undefined
  ready = false
  settled = false
  revealed = false
  if (!session.isCurrent(generation)) return
  phase = 'loading'
  failure = ''
  let instance: EpubNavigator | undefined
  try {
    if (!editionId) throw new Error('No edition id was provided to the reader.')
    // Read the stored position before the navigator exists: its first
    // `positionChanged` would otherwise persist the start of the book over it.
    const stored = await loadProgress(editionId)
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
    // Hand the saved locator to the navigator so it opens on that page: applying it
    // afterwards would render the cover first and flash it at the reader.
    const initialPosition = parseStoredLocator(stored?.epub?.locator)
    const host = container
    if (!host) throw new Error('The reader container is not available.')
    if (frameKeyHandler && frameWindow?.document) {
      frameWindow.document.removeEventListener('keydown', frameKeyHandler, true)
      frameKeyHandler = undefined
    }
    instance = new EpubNavigator(
      host,
      publication,
      {
        frameLoaded: (readiumWindow: Window) => {
          console.log('frameLoaded', readiumWindow?.document?.title)
          // A freshly loaded frame has its own comms channel; bind it before use.
          void syncFrameComms()
          frameWindow = readiumWindow
          const doc = readiumWindow?.document
          if (!doc) return

          installSemanticStyles(doc)

          const handler = (e: KeyboardEvent) => {
            if (!keyboard) return
            if (
              e.key === 'ArrowLeft' ||
              e.key === 'j' ||
              e.key === 'J' ||
              e.key === 'h' ||
              e.key === 'H'
            ) {
              e.preventDefault()
              previous()
            } else if (
              e.key === 'ArrowRight' ||
              e.key === 'k' ||
              e.key === 'K' ||
              e.key === 'l' ||
              e.key === 'L'
            ) {
              e.preventDefault()
              next()
            } else if (e.key === '+' || e.key === '=') {
              e.preventDefault()
              adjustFontSize('in')
            } else if (e.key === '-') {
              e.preventDefault()
              adjustFontSize('out')
            } else if (e.key === '0') {
              e.preventDefault()
              adjustFontSize('zero')
            }
          }
          frameKeyHandler = handler

          doc.addEventListener('keydown', handler, true)
        },
        positionChanged: (locator: Locator) => {
          ready = true
          latestLoc = locator
          installSemanticStylesEverywhere()
          // A selection belongs to the frame it was made in; drop it once the
          // reader moves so a later note never quotes stale text.
          ontextselect(null)
          savePosition()
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
          ontextselect({
            text: selected.text,
            locator: selected.locator ? JSON.stringify(selected.locator) : undefined,
          })
        },
        contentProtection: () => {},
        contextMenu: () => {},
        peripheral: () => {},
      },
      locators,
      initialPosition,
      {
        preferences: {},
        // Snapshot of the reactive presentation: the frames the navigator opens while
        // `load()` runs paint with it before any preference has been submitted.
        defaults: { ...presentation },
      },
    )

    // Clicking a highlight opens the annotation drawer; returning false leaves the
    // click to the navigator, so an unknown decoration never swallows a tap.
    instance.registerDecorationObserver(ANNOTATION_GROUP, {
      onDecorationActivated: (event) => {
        return onannotationactivate(event.decoration.id)
      },
    })

    // Frames are paged in while `load` runs and each one has to be re-bound before it
    // can answer: without this the first `show()` waits forever for an ack that a
    // frame with an uninitialised channel can never send.
    const bindFrames = setInterval(() => void syncFrameComms(instance), 150)
    try {
      await instance.load()
    } finally {
      clearInterval(bindFrames)
    }
    if (!session.isCurrent(generation)) {
      await instance.destroy().catch(() => {})
      return
    }
    navigator = instance
    // Frame modules are injected after Readium's initial ping; re-bind before use.
    await syncFrameComms(instance)
    // Preferences are submitted from one place only: the presentation effect, which runs
    // once `settled` flips below and carries ligatures with it. A second submit here would
    // be in flight alongside it, and two concurrent submits wedge the navigator's relayout.
    settled = true
    installSemanticStylesEverywhere()
    phase = 'ready'
    // The navigator opened on the stored locator, so there is nothing left to apply.
    revealed = true
  } catch (error) {
    if (instance) await instance.destroy().catch(() => {})
    if (!session.isCurrent(generation)) return
    navigator = undefined
    phase = 'error'
    failure = messageOf(error)
  }
}

/**
 * The Readium internals this component has to reach: frame comms and the navigation latch.
 * Both are private in `@readium/navigator`'s typings but are plain properties at runtime.
 */
type FrameCommsHandle = {
  ready: boolean
  channelId: string
  send: (key: string, data: unknown) => unknown
}
type FrameHandle = { msg?: FrameCommsHandle; iframe?: HTMLIFrameElement }
type NavigatorInternals = {
  currentLocator: EpubNavigator['currentLocator']
  go: EpubNavigator['go']
  goForward: EpubNavigator['goForward']
  goBackward: EpubNavigator['goBackward']
  _cframes?: (FrameHandle | undefined)[]
  _isNavigating?: boolean
}

/**
 * Re-bind each frame's comms channel.
 *
 * Readium pings a frame once, when the iframe element is created, but injects the
 * frame's module bundle (and with it the frame side of the channel) afterwards. That
 * first ping is therefore routinely lost: the frame ends up with no destination, so it
 * ignores every command we send and throws "Attempted to send comms message before
 * destination has been initialized" whenever it tries to speak. A frame that is already
 * bound answers nothing, so the wait is a timeout rather than a hang.
 */
/** Id of the `<style>` the reader owns inside a publication frame. */
const SEMANTIC_STYLE_ID = 'livtet-semantic'

/**
 * Write the reader's own styles into a publication frame.
 *
 * Every frame gets its own document, and those documents are rebuilt whenever
 * the navigator pages a resource in, so this runs per load. Re-applying is
 * harmless: the stylesheet is replaced in place rather than duplicated.
 */
function installSemanticStyles(doc: Document) {
  const existing = doc.getElementById(SEMANTIC_STYLE_ID)
  if (existing) {
    existing.textContent = SEMANTIC_STYLESHEET
    return
  }
  const style = doc.createElement('style')
  style.id = SEMANTIC_STYLE_ID
  style.textContent = SEMANTIC_STYLESHEET
  doc.head.append(style)
}

/**
 * Install the styles into every frame the navigator is holding.
 *
 * Readium reports a frame load through `frameLoaded(_cframes[0])` — always the
 * first frame — so the other pooled documents would never hear about the
 * stylesheet. Their documents are built when the resource is first paged in, so
 * this walks the pool whenever the reader moves.
 */
function installSemanticStylesEverywhere() {
  const frames = (navigator as unknown as NavigatorInternals | undefined)?._cframes ?? []
  for (const frame of frames) {
    const doc = frame?.iframe?.contentDocument
    if (doc) installSemanticStyles(doc)
  }
}

function syncFrameComms(target: EpubNavigator | undefined = navigator): Promise<void> {
  const frames = ((target as unknown as NavigatorInternals | undefined)?._cframes ?? []).filter(
    (frame): frame is FrameHandle => !!frame,
  )
  return Promise.all(
    frames.map(
      (frame) =>
        new Promise<void>((resolve) => {
          const comms = frame.msg
          if (!comms?.send) return resolve()
          let timer: ReturnType<typeof setTimeout>
          const finish = () => {
            window.removeEventListener('message', onMessage)
            clearTimeout(timer)
            resolve()
          }
          const onMessage = (event: MessageEvent) => {
            const data = event.data as { key?: string; _channel?: string } | undefined
            if (data?.key === '_pong' && data._channel === comms.channelId) finish()
          }
          timer = setTimeout(finish, 400)
          window.addEventListener('message', onMessage)
          comms.send('_ping', undefined)
        }),
    ),
  ).then(() => undefined)
}

/**
 * Move one page. Readium latches its navigator until the frame acknowledges the
 * command, so a command that is never answered (dead comms channel) would wedge every
 * later page turn; recover by re-syncing the channel and freeing the latch once.
 */
async function move(direction: 'forward' | 'backward', attempt = 0): Promise<void> {
  const target = navigator as unknown as NavigatorInternals | undefined
  if (!target?.currentLocator || !ready) {
    console.debug(
      'navigation blocked: navigator=' +
        !!target +
        ' locator=' +
        !!target?.currentLocator +
        ' ready=' +
        ready,
      { navigator: target, locator: target?.currentLocator },
    )
    return
  }
  const acked = await new Promise<boolean | undefined>((resolve) => {
    const timer = setTimeout(() => resolve(undefined), NAV_ACK_TIMEOUT)
    const done = (ok: boolean) => {
      clearTimeout(timer)
      resolve(ok)
    }
    if (direction === 'forward') target.goForward(false, done)
    else target.goBackward(false, done)
  })
  console.debug(`went to ${direction === 'forward' ? 'next' : 'previous'} page?`, acked ?? false)
  if (acked !== undefined) return
  // No ack: the frame that should have answered is gone (the navigator swaps in a new
  // iframe per resource) and the navigator is now latched on this command.
  console.debug(`readium ${direction} was never acknowledged; re-syncing frame comms`)
  target._isNavigating = false
  await syncFrameComms()
  if (attempt === 0) await move(direction, 1)
}

/** Turn back one page. */
export function previous() {
  void move('backward')
}

/** Turn forward one page. */
export function next() {
  void move('forward')
}

/**
 * Step the publication's text size, or put it back to Readium's default.
 *
 * The size is part of {@link presentation}, so changing it here is all it takes: the effect
 * above submits it with the colours already on screen.
 */
function adjustFontSize(direction: 'in' | 'out' | 'zero') {
  if (direction === 'zero') presentation.fontSize = DEFAULT_FONT_SIZE
  else presentation.fontSize = stepFontSize(presentation.fontSize, direction === 'in' ? 1 : -1)
}

onMount(() => {
  void load()
})

onDestroy(() => {
  session.invalidate()
  const instance = navigator
  navigator = undefined
  if (latestLoc) {
    void saveProgress(editionId, {
      kind: 'epub',
      epub: { locator: JSON.stringify(latestLoc) },
    })
  }
  if (frameWindow?.document && frameKeyHandler) {
    frameWindow.document.removeEventListener('keydown', frameKeyHandler, true)
    frameKeyHandler = undefined
  }
  void instance?.destroy().catch(() => {})
})
</script>

<div class="frame">
  {#if phase === 'loading'}
    <p class="muted">Loading…</p>
  {:else if phase === 'error'}
    <div class="error">
      <p>Could not open this book: {failure}</p>
      <ActionButton onclick={reload}>Retry</ActionButton>
    </div>
  {/if}

  <!-- Pane grows this; the Readium iframe scrolls inside it. -->
  <div
    class="viewport"
    data-scroll-region
    bind:this={container}
    style:visibility={revealed ? 'visible' : 'hidden'}
  ></div>
</div>
<div style="display: none">
  <a href="https://www.olamaelcu.net"></a>
  <a href="#"></a>
</div>

<style>
  .frame {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
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
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    width: 100% !important;
    max-height: 82.5vh;
    margin: 2.5vh auto;

    :global(iframe.readium-navigator-iframe) {
      position: absolute;
      inset: 0;
      width: 100%;
      height: 100%;
      max-width: 100%;
      max-height: 100%;
      margin: 1rem 0;
      border: 0;
    }
  }
</style>
