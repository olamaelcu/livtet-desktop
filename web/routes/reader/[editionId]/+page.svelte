<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { page } from '$app/state'
import ActionButton from '../../../lib/components/ActionButton.svelte'
import { readerKeys } from '../../../lib/query/keys'
import {
  chapterAt,
  formatTimestamp,
  loadListeningProgress,
  loadReaderPublication,
  saveListeningProgress,
} from '../../../lib/reader'

const SAVE_EVERY_SECONDS = 15
const SKIP_SECONDS = 30
const RATES = [1, 1.25, 1.5, 1.75, 2]

const editionId = $derived(page.params.editionId ?? '')

const publicationQuery = createQuery(() => ({
  queryKey: readerKeys.publication(editionId),
  queryFn: () => loadReaderPublication(editionId),
  enabled: editionId !== '',
}))
const progressQuery = createQuery(() => ({
  queryKey: readerKeys.progress(editionId),
  queryFn: () => loadListeningProgress(editionId),
  enabled: editionId !== '',
}))

const publication = $derived(
  publicationQuery.data?.kind === 'Audiobook' ? publicationQuery.data : null,
)
const chapters = $derived(publication?.chapters ?? [])
const duration = $derived(publication?.duration_seconds ?? 0)

let audio = $state<HTMLAudioElement | undefined>()
let position = $state(0)
let rate = $state(1)
let resumed = $state(false)
let lastSaved = $state(0)

const currentChapter = $derived(chapterAt(chapters, position))

async function persist(force = false): Promise<void> {
  if (!editionId) return
  if (!force && Math.abs(position - lastSaved) < 1) return
  try {
    await saveListeningProgress(editionId, position)
    lastSaved = position
  } catch {
    toast.error('Could not save listening progress')
  }
}

function onTimeUpdate(): void {
  if (!audio) return
  position = audio.currentTime
  if (position - lastSaved >= SAVE_EVERY_SECONDS) void persist()
}

function onLoadedMetadata(): void {
  if (!audio || resumed) return
  resumed = true
  const saved = progressQuery.data?.position_seconds ?? 0
  const total = audio.duration || duration
  if (saved > 5 && saved < total) {
    audio.currentTime = saved
    position = saved
  }
  lastSaved = position
}

function seekTo(seconds: number): void {
  if (!audio) return
  audio.currentTime = Math.min(Math.max(0, seconds), audio.duration || duration)
  position = audio.currentTime
  void audio.play().catch(() => toast.error('Could not start playback'))
}

function chooseRate(event: Event): void {
  const value = Number((event.target as HTMLSelectElement).value)
  rate = RATES.includes(value) ? value : 1
  if (audio) audio.playbackRate = rate
}

$effect(() => {
  const persistOnHide = () => void persist(true)
  window.addEventListener('pagehide', persistOnHide)
  return () => window.removeEventListener('pagehide', persistOnHide)
})
</script>

<div class="reader">
  {#if publicationQuery.isPending}
    <p class="muted">Loading…</p>
  {:else if publicationQuery.isError}
    <p class="error">Could not load this book for playback.</p>
  {:else if !publication}
    <p class="muted">No playable audiobook found for this edition.</p>
  {:else}
    <header class="header">
      <h2 class="title">{publication.title ?? 'Untitled'}</h2>
      <p class="muted">
        {formatTimestamp(position)} / {formatTimestamp(duration)}
      </p>
    </header>

    <audio
      bind:this={audio}
      class="player"
      src={publication.audio_url}
      controls
      preload="metadata"
      ontimeupdate={onTimeUpdate}
      onloadedmetadata={onLoadedMetadata}
      onpause={() => void persist(true)}
      onended={() => void persist(true)}
    ></audio>

    <div class="controls">
      <ActionButton onclick={() => seekTo(position - SKIP_SECONDS)}>
        <wa-icon name="rotate-ccw"></wa-icon>
        {SKIP_SECONDS}s
      </ActionButton>
      <ActionButton onclick={() => seekTo(position + SKIP_SECONDS)}>
        <wa-icon name="rotate-cw"></wa-icon>
        {SKIP_SECONDS}s
      </ActionButton>
      <wa-select
        size="s"
        class="rate"
        aria-label="Playback speed"
        value={String(rate)}
        onchange={chooseRate}
      >
        {#each RATES as option (option)}
          <wa-option value={String(option)}>{option}×</wa-option>
        {/each}
      </wa-select>
    </div>

    {#if chapters.length > 0}
      <section class="section">
        <h3 class="section-title">Chapters</h3>
        <ol class="chapters">
          {#each chapters as chapter, index (chapter.audio_start)}
            <li>
              <ActionButton
                variant={index === currentChapter ? 'brand' : undefined}
                onclick={() => seekTo(chapter.audio_start)}
              >
                <span class="chapter-name">{chapter.name}</span>
                <span class="muted">{formatTimestamp(chapter.audio_start)}</span>
              </ActionButton>
            </li>
          {/each}
        </ol>
      </section>
    {/if}
  {/if}
</div>

<style>
  .reader {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-m);
    max-width: 40rem;
    margin: 0 auto;
    padding: var(--wa-space-l);
  }
  .header {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }
  .title {
    margin: 0;
    font-size: 1.25rem;
  }
  .muted {
    color: var(--wa-color-text-quiet);
  }
  .error {
    color: var(--wa-color-danger-fill-loud);
  }
  .player {
    width: 100%;
  }
  .controls {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
  }
  .rate {
    min-width: 6rem;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }
  .section-title {
    margin: 0;
    font-size: 1rem;
  }
  .chapters {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chapter-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
