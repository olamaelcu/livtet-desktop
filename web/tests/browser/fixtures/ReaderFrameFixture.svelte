<script lang="ts">
import { Locator } from '@readium/shared'
import ReaderFrame from '$lib/components/ReaderFrame.svelte'
import type { Chapter } from '$lib/reader/chapters'
import type { Annotation } from '$lib/reader/progress'

interface Props {
  editionId?: string
  annotations?: Annotation[]
  /** Whether the frame may handle page-turn keys; false while a dialog has focus. */
  keyboard?: boolean
  events: string[]
}

let { editionId = 'ed-1', annotations = [], keyboard = true, events }: Props = $props()

// The frame's imperative surface, as the pub page uses it through `bind:this`.
type FrameApi = {
  next: () => void
  previous: () => void
  reload: () => void
  go: (locator: Locator) => void
  currentLocator: () => { href: string } | undefined
  quoteAtPosition: () => unknown
}

let frame = $state<FrameApi | undefined>(undefined)

// Outputs the frame publishes while the reader moves.
let ready = $state(false)
let title = $state('')
let progress = $state('')
let location = $state('')
let progression = $state(0)
let chapters = $state<Chapter[]>([])

// `currentLocator()` reads straight through to the navigator, so it is sampled
// on demand rather than rendered from reactive state.
let sampledHref = $state('')
let sampledQuote = $state('')

/** Somewhere other than the page on screen, for the imperative `go`. */
const elsewhere = Locator.deserialize({
  href: '/ch1.xhtml',
  type: 'application/xhtml+xml',
  locations: { position: 2, progression: 0.6 },
}) as Locator
</script>

<div class="controls">
  <button data-next type="button" onclick={() => frame?.next()}>next</button>
  <button data-previous type="button" onclick={() => frame?.previous()}>previous</button>
  <button data-reload type="button" onclick={() => frame?.reload()}>reload</button>
  <button
    data-sample-current
    type="button"
    onclick={() => (sampledHref = frame?.currentLocator()?.href ?? 'none')}>current</button
  >
  <button
    data-quote
    type="button"
    onclick={() => (sampledQuote = JSON.stringify(frame?.quoteAtPosition() ?? null))}>quote</button
  >
  <button data-go type="button" onclick={() => frame?.go(elsewhere)}>go</button>
</div>

<span data-ready>{ready}</span>
<span data-title>{title}</span>
<span data-progress>{progress}</span>
<span data-location>{location}</span>
<span data-progression>{progression}</span>
<span data-chapters>{chapters.map((chapter) => chapter.title).join('|')}</span>
<span data-current-href>{sampledHref}</span>
<span data-quote-result>{sampledQuote}</span>

<ReaderFrame
  bind:this={frame}
  {editionId}
  {annotations}
  {keyboard}
  bind:ready
  bind:title
  bind:progress
  bind:location
  bind:progression
  bind:chapters
  ontextselect={(selection: { text: string; locator?: string } | null) =>
    events.push(selection ? `select:${selection.text}` : 'select:null')}
  onannotationactivate={(id: string) => {
    events.push(`activate:${id}`)
    return true
  }}
  onchapters={() => events.push('chapters')}
/>
