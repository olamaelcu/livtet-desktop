import type * as NavigatorModule from '@readium/navigator'
import { Locator } from '@readium/shared'
import { afterEach, beforeEach, expect, test, vi } from 'vitest'
import { render } from 'vitest-browser-svelte'
import { ANNOTATION_GROUP, SEMANTIC_GROUP } from '$lib/reader/decorations'
import { DEFAULT_FONT_SIZE } from '$lib/reader/fontSize'
import type * as ProgressModule from '$lib/reader/progress'
import type { Annotation } from '$lib/reader/progress'
import ReaderFrameFixture from './fixtures/ReaderFrameFixture.svelte'

const XHTML = 'application/xhtml+xml'

/** Where the frame keeps the reader's text size; a mount reads it, stepping rewrites it. */
const FONT_SIZE_KEY = 'livtet.reader.fontSize'

/** Minimal RWPM manifest the frame can open: a title and one spine item. */
const MANIFEST = {
  metadata: { title: 'Test Book', identifier: 'urn:uuid:1', language: 'en' },
  readingOrder: [{ href: '/ch1.xhtml', type: XHTML }],
  resources: [],
}

function locator(position?: number, progression?: number): Locator {
  return Locator.deserialize({
    href: '/ch1.xhtml',
    type: XHTML,
    locations: { position, progression },
  }) as Locator
}

interface FrameComms {
  channelId: string
  send: (key: string, data: unknown) => unknown
}

interface PooledFrame {
  iframe?: HTMLIFrameElement
  msg?: FrameComms
}

/** The shape of the navigator the component drives, as the test needs to poke it. */
interface NavigatorMock {
  host: HTMLElement
  callbacks: {
    frameLoaded: (frameWindow: Window) => void
    positionChanged: (locator: Locator) => void
    textSelected: (selection: { text: string; locator?: Locator }) => void
  }
  locators: unknown[]
  initialPosition: Locator | undefined
  options: { preferences: unknown; defaults: Record<string, unknown> }
  currentLocator: Locator | undefined
  timeline: { items: { position?: number }[] }
  _cframes: PooledFrame[]
  _isNavigating: boolean
}

const mocks = vi.hoisted(() => ({
  instances: [] as unknown[],
  load: vi.fn(),
  destroy: vi.fn(),
  submitPreferences: vi.fn(),
  applyDecorations: vi.fn(),
  registerDecorationObserver: vi.fn(),
  go: vi.fn(),
  goForward: vi.fn(),
  goBackward: vi.fn(),
  loadEpub: vi.fn(),
  loadProgress: vi.fn(),
  getReaderPosition: vi.fn(),
  saveReaderPosition: vi.fn(),
  openUrl: vi.fn(),
}))

// Readium's navigator owns a real iframe, a worker and a manifest fetch. The
// frame's own behaviour — what it does with the callbacks it hands over, the
// keys it binds inside the frame, the state it publishes — is testable against
// a stand-in that records its construction and lets the test fire those
// callbacks. What the navigator does internally is Readium's to test.
vi.mock('@readium/navigator', async (importOriginal) => {
  // Everything else the module exports stays real: the reader's font-size
  // stepping reads `fontSizeRangeConfig` from it.
  const original = await importOriginal<typeof NavigatorModule>()
  class EpubPreferences {
    constructor(values: Record<string, unknown>) {
      Object.assign(this, values)
    }
  }

  class EpubNavigator {
    host: unknown
    publication: unknown
    callbacks: unknown
    locators: unknown
    initialPosition: unknown
    options: unknown
    currentLocator: unknown = undefined
    timeline = { items: [] as { position?: number }[] }
    _cframes: unknown[] = []
    _isNavigating = false

    constructor(
      host: unknown,
      publication: unknown,
      callbacks: unknown,
      locators: unknown,
      initialPosition: unknown,
      options: unknown,
    ) {
      this.host = host
      this.publication = publication
      this.callbacks = callbacks
      this.locators = locators
      this.initialPosition = initialPosition
      this.options = options
      mocks.instances.push(this)
    }

    load() {
      return mocks.load()
    }

    destroy() {
      return mocks.destroy()
    }

    submitPreferences(preferences: unknown) {
      return mocks.submitPreferences(preferences)
    }

    applyDecorations(...args: unknown[]) {
      return mocks.applyDecorations(...args)
    }

    registerDecorationObserver(...args: unknown[]) {
      return mocks.registerDecorationObserver(...args)
    }

    go(...args: unknown[]) {
      return mocks.go(...args)
    }

    goForward(...args: unknown[]) {
      return mocks.goForward(...args)
    }

    goBackward(...args: unknown[]) {
      return mocks.goBackward(...args)
    }
  }

  return { ...original, EpubNavigator, EpubPreferences }
})

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: mocks.openUrl }))

vi.mock('$lib/bindings', () => ({
  commands: {
    getReaderPosition: mocks.getReaderPosition,
    saveReaderPosition: mocks.saveReaderPosition,
  },
}))

vi.mock('$lib/reader/read', () => ({
  loadEpubReaderPublication: mocks.loadEpub,
  readReaderResource: vi.fn(),
}))

// Only the stored position is stubbed: the real `debouncedSave`/`saveProgress`
// pair then runs the save the frame schedules, through the mocked commands.
vi.mock('$lib/reader/progress', async (importOriginal) => {
  const original = await importOriginal<typeof ProgressModule>()
  return { ...original, loadProgress: mocks.loadProgress }
})

function navigator(): NavigatorMock {
  expect(mocks.instances, 'the frame must construct a navigator').not.toHaveLength(0)
  return mocks.instances[0] as NavigatorMock
}

function text(container: HTMLElement, selector: string): string {
  return container.querySelector(selector)?.textContent?.trim() ?? ''
}

interface SubmittedPreferences {
  fontSize: number
  ligatures: boolean
}

/** Spine position of every reading position the frame wrote, oldest first. */
function savedPositions(): (number | undefined)[] {
  return mocks.saveReaderPosition.mock.calls.map((call) => {
    const stored = JSON.parse(call[2] as string) as { locations?: { position?: number } }
    return stored.locations?.position
  })
}

/** A publication frame's document, detached from the test document. */
function frameDocument(): Document {
  return document.implementation.createHTMLDocument('ch1')
}

function asWindow(doc: Document): Window {
  return { document: doc } as unknown as Window
}

async function mount(props: { annotations?: Annotation[]; keyboard?: boolean } = {}): Promise<{
  container: HTMLElement
  events: string[]
  open: () => void
}> {
  const events: string[] = []
  const view = await render(ReaderFrameFixture, { props: { events, ...props } })
  mounted.push(view.unmount)
  return { container: view.container, events, open: () => view.unmount() }
}

// A frame left mounted keeps its 100 ms poll and its debounced save alive,
// which would leak into the next test's call counts.
const mounted: (() => void)[] = []

afterEach(() => {
  for (const unmount of mounted.splice(0)) unmount()
  // The text size is a real preference now, so a test that steps it would otherwise
  // leave the next mount on that size instead of Readium's default.
  localStorage.removeItem(FONT_SIZE_KEY)
})

/** Wait for the navigator to be constructed and loaded, i.e. the frame is ready. */
async function settledFrame(container: HTMLElement): Promise<NavigatorMock> {
  await expect.poll(() => mocks.instances.length).toBe(1)
  // The component only assigns its `navigator` once `load()` resolves, and the
  // effects and callbacks below reach the navigator through that state.
  await expect.poll(() => container.querySelector('.muted')).toBeNull()
  const instance = navigator()
  instance.currentLocator = locator(1, 0.5)
  instance.timeline = { items: [{ position: 1 }, { position: 2 }] }
  instance.callbacks.positionChanged(instance.currentLocator)
  await expect.poll(() => text(container, '[data-ready]')).toBe('true')
  return instance
}

beforeEach(() => {
  mocks.instances.length = 0
  vi.clearAllMocks()
  mocks.load.mockResolvedValue(undefined)
  mocks.destroy.mockResolvedValue(undefined)
  mocks.submitPreferences.mockResolvedValue(undefined)
  mocks.applyDecorations.mockReturnValue(undefined)
  mocks.registerDecorationObserver.mockReturnValue(undefined)
  mocks.go.mockReturnValue(undefined)
  mocks.goForward.mockImplementation((_locator: unknown, done?: (ok: boolean) => void) =>
    done?.(true),
  )
  mocks.goBackward.mockImplementation((_locator: unknown, done?: (ok: boolean) => void) =>
    done?.(true),
  )
  mocks.loadProgress.mockResolvedValue(null)
  mocks.getReaderPosition.mockResolvedValue({ status: 'ok', data: null })
  mocks.saveReaderPosition.mockResolvedValue({ status: 'ok', data: null })
  mocks.openUrl.mockResolvedValue(undefined)
  mocks.loadEpub.mockResolvedValue({ manifest: MANIFEST, positions: [] })
})

test('opens the publication and takes its title from the manifest', async () => {
  const { container } = await mount()

  expect(container.querySelector('.muted')?.textContent).toContain('Loading')
  await expect.poll(() => text(container, '[data-title]')).toBe('Test Book')
  await expect.poll(() => container.querySelector('.muted')).toBeNull()
  // Revealed only once the navigator has opened, so the cover never flashes.
  await expect
    .poll(() => container.querySelector<HTMLElement>('.viewport')?.style.visibility)
    .toBe('visible')
})

test('opens on the stored position instead of the first page', async () => {
  const stored = locator(7, 0.4)
  mocks.loadProgress.mockResolvedValue({ kind: 'epub', epub: { locator: JSON.stringify(stored) } })
  mocks.loadEpub.mockResolvedValue({
    manifest: MANIFEST,
    positions: [{ href: '/ch1.xhtml', type: XHTML, locations: { position: 7 } }],
  })

  await mount()

  await expect.poll(() => mocks.instances.length).toBe(1)
  const initial = navigator().initialPosition
  expect(initial?.href).toBe('/ch1.xhtml')
  expect(initial?.locations?.position).toBe(7)
  // The generated position list travels with it, so Readium can page by position.
  expect(navigator().locators).toHaveLength(1)
})

test('publishes the page on screen and persists it', async () => {
  const { container, events } = await mount()
  await settledFrame(container)

  // One spine position covers both pages here, so the fraction comes from the
  // in-resource progression: (position 1 - 1 + 0.5) / 2 positions.
  await expect.poll(() => text(container, '[data-progress]')).toBe('25.00%')
  expect(text(container, '[data-progression]')).toBe('0.25')
  expect(text(container, '[data-location]')).not.toBe('')
  // A selection belongs to the frame it was made in.
  expect(events).toContain('select:null')

  await expect.poll(() => savedPositions()).toEqual([1])
  expect(mocks.saveReaderPosition.mock.calls[0]?.[0]).toBe('ed-1')
  expect(mocks.saveReaderPosition.mock.calls[0]?.[1]).toBe('epub')
})

test('reports a frame whose publication cannot be read', async () => {
  mocks.loadEpub.mockResolvedValue({ manifest: {}, positions: [] })

  const { container } = await mount()

  await expect.poll(() => container.querySelector('.error')?.textContent).toContain('is invalid')
})

test('reports a failed load and retries it from the error panel', async () => {
  mocks.loadEpub.mockRejectedValueOnce(new Error('manifest unreadable'))

  const { container } = await mount()

  await expect
    .poll(() => container.querySelector('.error')?.textContent)
    .toContain('manifest unreadable')

  container.querySelector<HTMLButtonElement>('[data-reload]')?.click()

  await expect.poll(() => mocks.loadEpub.mock.calls.length).toBe(2)
  await expect.poll(() => container.querySelector('.error')).toBeNull()
})

test('turns pages from the keys it binds inside the frame', async () => {
  const { container, events } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', cancelable: true }))
  doc.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', cancelable: true }))

  // ArrowRight turns forward; `j` is one of the four backward keys.
  await expect.poll(() => mocks.goForward.mock.calls.length).toBe(1)
  expect(mocks.goBackward.mock.calls.length).toBe(1)
  expect(events).toContain('select:null')
})

test('lists the spine as chapters, titled from the table of contents', async () => {
  mocks.loadEpub.mockResolvedValue({
    manifest: {
      ...MANIFEST,
      readingOrder: [
        { href: '/cover.xhtml', type: XHTML },
        { href: '/ch1.xhtml', type: XHTML },
      ],
      toc: [
        { href: '/cover.xhtml', title: 'Cover' },
        { href: '/ch1.xhtml', title: 'One' },
      ],
    },
    positions: [],
  })

  const { container } = await mount()
  await settledFrame(container)

  // Reading-order links carry no title, so this is the join working: titles come
  // from the separate toc collection, in spine order.
  await expect.poll(() => text(container, '[data-chapters]')).toBe('Cover|One')
})

test('toggles the chapter list from the key it binds inside the frame', async () => {
  const { container, events } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', cancelable: true }))

  // A key pressed in the frame's own document never reaches the page's
  // listener, so the frame has to forward this one itself.
  await expect.poll(() => events).toContain('chapters')
})

test('leaves the frame keys inert while a dialog has focus', async () => {
  const { container } = await mount({ keyboard: false })
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  const event = new KeyboardEvent('keydown', { key: 'ArrowLeft', cancelable: true })
  doc.dispatchEvent(event)

  // The handler is synchronous: nothing is scheduled for later, so an empty
  // call list here means the key was refused, not merely delayed.
  expect(mocks.goBackward).not.toHaveBeenCalled()
  expect(event.defaultPrevented).toBe(false)
})

test('sends a selection from the frame to the page', async () => {
  const { container, events } = await mount()
  const instance = await settledFrame(container)

  instance.callbacks.textSelected({ text: 'a quoted line', locator: locator(2, 0.3) })

  expect(events).toContain('select:a quoted line')
  const last = events[events.length - 1] ?? ''
  expect(last).toBe('select:a quoted line')
})

test('steps the text size and submits it to the navigator', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  await expect.poll(() => mocks.submitPreferences.mock.calls.length).toBeGreaterThan(0)
  expect(submittedPreferences()[0]).toMatchObject({
    fontSize: DEFAULT_FONT_SIZE,
    ligatures: true,
  })

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: '+', cancelable: true }))
  await expect
    .poll(() => submittedPreferences().at(-1)?.fontSize)
    .toBeGreaterThan(DEFAULT_FONT_SIZE)

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: '0', cancelable: true }))
  await expect.poll(() => submittedPreferences().at(-1)?.fontSize).toBe(DEFAULT_FONT_SIZE)
})

test('steps the text size back down', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))
  await expect.poll(() => submittedPreferences().length).toBeGreaterThan(0)

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: '-', cancelable: true }))

  await expect.poll(() => submittedPreferences().at(-1)?.fontSize).toBeLessThan(DEFAULT_FONT_SIZE)
})

test('opens on the text size the reader last chose', async () => {
  localStorage.setItem(FONT_SIZE_KEY, '1.5')

  const { container } = await mount()
  const instance = await settledFrame(container)
  instance.callbacks.frameLoaded(asWindow(frameDocument()))

  // The restored size has to reach Readium in the first submit, before the reader can
  // press a key: anything later would paint the default first.
  await expect.poll(() => submittedPreferences()[0]?.fontSize).toBe(1.5)
})

test('keeps the text size the reader chose for the next book', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))
  await expect.poll(() => submittedPreferences().length).toBeGreaterThan(0)

  doc.dispatchEvent(new KeyboardEvent('keydown', { key: '+', cancelable: true }))

  await expect.poll(() => localStorage.getItem(FONT_SIZE_KEY)).toBe('1.05')
})

test('samples the current locator and the text anchor at it', async () => {
  const { container } = await mount()
  await settledFrame(container)

  container.querySelector<HTMLButtonElement>('[data-sample-current]')?.click()
  await expect.poll(() => text(container, '[data-current-href]')).toBe('/ch1.xhtml')

  // Nothing is rendered inside the viewport, so there is no text to quote: the
  // page must not receive a tag for an anchor it cannot resolve.
  container.querySelector<HTMLButtonElement>('[data-quote]')?.click()
  await expect.poll(() => text(container, '[data-quote-result]')).toBe('null')
})

test('stops moving once the navigator reports the requested page', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  mocks.go.mockImplementation((target: unknown, _instant: unknown, done?: () => void) => {
    instance.currentLocator = target as Locator
    done?.()
  })

  container.querySelector<HTMLButtonElement>('[data-go]')?.click()

  await expect.poll(() => mocks.go.mock.calls.length).toBe(1)
  // Arrival is proven by the locator matching, not by the navigator's ack.
  expect(mocks.go.mock.calls[0]?.[0]).toMatchObject({ href: '/ch1.xhtml' })
  expect(instance._isNavigating).toBe(false)
})

test('frees the navigation latch when a move never arrives', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const send = vi.fn()
  instance._cframes = [{ msg: { channelId: 'channel-1', send } }]
  // The navigator keeps the locator it had and latches itself on the command.
  mocks.go.mockImplementation(() => undefined)
  instance._isNavigating = true

  container.querySelector<HTMLButtonElement>('[data-go]')?.click()

  await expect.poll(() => mocks.go.mock.calls.length, { timeout: 4000 }).toBe(2)
  expect(send).toHaveBeenCalledWith('_ping', undefined)
  expect(instance._isNavigating).toBe(false)
})

/** Every preference set the frame has handed to the navigator, oldest first. */
function submittedPreferences(): SubmittedPreferences[] {
  const calls = mocks.submitPreferences.mock.calls
  return calls.map((call) => call[0] as SubmittedPreferences)
}

test('draws notes and semantic tints once the navigator has settled', async () => {
  const note: Annotation = {
    id: 'note-1',
    text: 'a note',
    quote: 'highlighted words',
    locator: JSON.stringify({ ...locator(1, 0.2).serialize(), text: { highlight: 'highlighted' } }),
  }
  const { container } = await mount({ annotations: [note] })

  await settledFrame(container)

  const groups = mocks.applyDecorations.mock.calls.map((call) => call[1])
  expect(groups).toContain(ANNOTATION_GROUP)
  expect(groups).toContain(SEMANTIC_GROUP)
  const [decorations] = mocks.applyDecorations.mock.calls.find(
    (call) => call[1] === ANNOTATION_GROUP,
  ) as [unknown[]]
  expect(decorations.length).toBe(1)
})

test('routes a clicked highlight to the page and honours its verdict', async () => {
  const { container } = await mount()
  await settledFrame(container)

  const observer = mocks.registerDecorationObserver.mock.calls[0] as [
    string,
    { onDecorationActivated: (event: { decoration: { id: string } }) => boolean },
  ]
  expect(observer[0]).toBe(ANNOTATION_GROUP)
  expect(observer[1].onDecorationActivated({ decoration: { id: 'note-1' } })).toBe(true)
})

test('holds an external link for confirmation, then opens it in the browser', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  const anchor = doc.createElement('a')
  anchor.href = 'https://example.com/article'
  anchor.textContent = 'somewhere else'
  doc.body.append(anchor)

  const click = new MouseEvent('click', { bubbles: true, cancelable: true })
  anchor.dispatchEvent(click)

  // Following it in place would load a remote page into the reader's frame.
  expect(click.defaultPrevented).toBe(true)
  await expect.poll(() => text(container, '.url')).toBe('https://example.com/article')

  container.querySelector<HTMLButtonElement>('.confirm')?.click()

  await expect.poll(() => mocks.openUrl.mock.calls.length).toBe(1)
  expect(mocks.openUrl).toHaveBeenCalledWith('https://example.com/article')
})

test('cancelling the confirmation dismisses the link', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  const anchor = doc.createElement('a')
  anchor.href = 'https://example.com/article'
  doc.body.append(anchor)
  anchor.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
  await expect.poll(() => text(container, '.url')).toBe('https://example.com/article')

  container.querySelector<HTMLButtonElement>('.actions button')?.click()

  await expect.poll(() => text(container, '.url')).toBe('')
  expect(mocks.openUrl).not.toHaveBeenCalled()
})

test('leaves in-book links to the navigator', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const doc = frameDocument()
  instance.callbacks.frameLoaded(asWindow(doc))

  const anchor = doc.createElement('a')
  anchor.setAttribute('href', '#note-1')
  doc.body.append(anchor)

  const click = new MouseEvent('click', { bubbles: true, cancelable: true })
  anchor.dispatchEvent(click)

  expect(click.defaultPrevented).toBe(false)
  expect(text(container, '.url')).toBe('')
})

test('installs the reader stylesheet into the frame it is handed', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)

  const reported = frameDocument()
  instance.callbacks.frameLoaded(asWindow(reported))

  // The reader's own stylesheet, carrying the pagebreak tick it installs.
  expect(reported.getElementById('livtet-semantic')?.textContent).toContain('pagebreak')
})

test('prepares the pooled frames Readium does not report on load', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)

  // Readium pools one iframe per resource and reports only the first through
  // `frameLoaded`, so a document built later is prepared when the reader moves.
  const pooled = document.createElement('iframe')
  container.querySelector('.viewport')?.append(pooled)
  await expect.poll(() => pooled.contentDocument).not.toBeNull()
  const pooledDoc = pooled.contentDocument

  instance.callbacks.positionChanged(locator(1, 0.5))

  expect(pooledDoc?.getElementById('livtet-semantic')).not.toBeNull()
  pooled.remove()
})

test('saves the last position once when the reader closes', async () => {
  const { container, open } = await mount()
  const instance = await settledFrame(container)
  await expect.poll(() => savedPositions()).toEqual([1])

  // A page turn queues a debounced save; closing before it fires must not let it
  // run after teardown, on top of the position `onDestroy` writes synchronously.
  instance.currentLocator = locator(2, 0.6)
  instance.callbacks.positionChanged(instance.currentLocator)
  open()

  expect(savedPositions()).toEqual([1, 2])
  await expect.poll(() => mocks.destroy.mock.calls.length).toBe(1)
})

test('re-syncs a frame whose page turn is never acknowledged', async () => {
  const { container } = await mount()
  const instance = await settledFrame(container)
  const send = vi.fn()
  instance._cframes = [{ msg: { channelId: 'channel-1', send } }]
  // A dead comms channel never answers, and the navigator stays latched on the
  // command until the frame frees it.
  mocks.goForward.mockImplementation(() => undefined)

  container.querySelector<HTMLButtonElement>('[data-next]')?.click()

  await expect.poll(() => mocks.goForward.mock.calls.length, { timeout: 3000 }).toBe(2)
  expect(send).toHaveBeenCalledWith('_ping', undefined)
  expect(instance._isNavigating).toBe(false)
})

test('destroys a navigator whose load was superseded by a reload', async () => {
  // The first load stalls inside `navigator.load()` while a reload supersedes
  // it: the frame that never gets to show its book must not keep its navigator.
  const stalledLoad = Promise.withResolvers<void>()
  mocks.load.mockImplementationOnce(() => stalledLoad.promise)
  const { container } = await mount()
  await expect.poll(() => mocks.instances.length).toBe(1)

  container.querySelector<HTMLButtonElement>('[data-reload]')?.click()
  await expect.poll(() => mocks.instances.length).toBe(2)

  stalledLoad.resolve()

  // One destroy for the superseded navigator, one for the frame on teardown.
  await expect.poll(() => mocks.destroy.mock.calls.length).toBeGreaterThanOrEqual(1)
  expect(container.querySelector('.muted')).toBeNull()
})
