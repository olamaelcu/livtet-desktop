<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { toast } from 'svelte-sonner'
import { page } from '$app/state'
import AudioPlayer from '$lib/components/AudioPlayer.svelte'
import ChapterList from '$lib/components/ChapterList.svelte'
import PlayerProgress from '$lib/components/PlayerProgress.svelte'
import ScrollRegion from '$lib/layout/ScrollRegion.svelte'
import { readerKeys } from '$lib/query/keys'
import {
  formatTimestamp,
  loadListeningProgress,
  loadReaderPublication,
  saveListeningProgress,
} from '$lib/reader'
import { coverUrlFor, loadEditionCovers, loadEditionDetail } from '$lib/search'
import ReaderLayout from '../../ReaderLayout.svelte'

const SAVE_EVERY_SECONDS = 15

const editionId = $derived(page.params.editionId ?? '')
const covers = createQuery(() => ({
  queryKey: ['editionCover', editionId],
  queryFn: () => loadEditionCovers([editionId]),
  enabled: editionId !== '',
}))
const coverUrl = $derived(coverUrlFor(covers.data?.[0]?.cover_path))

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
const editionDetailQuery = createQuery(() => ({
  queryKey: ['editionDetail', editionId],
  queryFn: () => loadEditionDetail(editionId),
  enabled: editionId !== '',
}))

const publication = $derived(
  publicationQuery.data?.kind === 'Audiobook' ? publicationQuery.data : null,
)
const chapters = $derived(publication?.chapters ?? [])
const duration = $derived(publication?.duration_seconds ?? 0)
const contributors = $derived(editionDetailQuery.data?.authors ?? [])

let audio = $state<HTMLAudioElement | undefined>()
let position = $state(0)
let rate = $state(1)
let isPlaying = $derived(!audio?.paused)
let lastSaved = $state(0)
let resumed = $state(false)

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
  const max = audio.duration || duration
  const newTime = Math.min(Math.max(0, seconds), max)
  audio.currentTime = newTime
  position = newTime
}

function onSeek(event: CustomEvent<{ position: number }>): void {
  if (!audio) return
  audio.currentTime = event.detail.position
  position = event.detail.position
}

function onVolumeChange(event: CustomEvent<{ volume: number }>): void {
  if (!audio) return
  audio.volume = event.detail.volume
}

function onRateChange(event: CustomEvent<{ rate: number }>): void {
  rate = event.detail.rate
  if (audio) audio.playbackRate = rate
}

function onTogglePlay(): void {
  if (audio?.paused) {
    audio.play().catch((e) => {
      console.error('Audio playback failed', e)
      if (e.toString().includes('AbortError')) {
        // Handle abort error specifically if needed
      } else {
        toast.error('Could not start playback')
      }
    })
  } else {
    audio?.pause()
  }
}

$effect(() => {
  const persistOnHide = () => void persist(true)
  window.addEventListener('pagehide', persistOnHide)
  return () => window.removeEventListener('pagehide', persistOnHide)
})
</script>

<ReaderLayout>
  <!-- Hidden audio element for native playback engine -->
  <audio
    bind:this={audio}
    class="wa-player-audio"
    src={publication?.audio_url}
    preload="metadata"
    ontimeupdate={onTimeUpdate}
    onloadedmetadata={onLoadedMetadata}
    onpause={() => void persist(true)}
    onended={() => void persist(true)}
    onratechange={() => void (rate = audio!.playbackRate)}
  ></audio>

  <ScrollRegion>
    <div class="reader">
      {#if publicationQuery.isPending}
        <wa-callout>Loading the book&hellip;</wa-callout>
      {:else if publicationQuery.isError}
        <wa-callout>Couldn't load this book for playback.</wa-callout>
      {:else if !publication}
        <wa-callout>No playable audiobook found for this edition.</wa-callout>
      {:else}
        {#if coverUrl}
          <img class="cover" src={coverUrl} alt="" />
        {/if}
        <header class="header header-row">
          <h2 class="title">{publication.title ?? 'Untitled'}</h2>
          {#if contributors.length > 0}
            <ul class="contributors">
              {#each contributors.slice(0, 1) as contributor (contributor.name + contributor.role)}
                <li class="contributor">
                  <span>{contributor.name}</span>
                  <wa-badge title={contributor.role}>{contributor.role_label}</wa-badge>
                </li>
              {/each}
            </ul>
          {/if}
        </header>

        <!-- Custom player UI -->
        <div class="custom-audio-player">
          <PlayerProgress
            {position}
            {duration}
            formatTime={formatTimestamp}
            on:seek={onSeek}
          />

          <!-- Controls & Volume -->
          <AudioPlayer
            {audio}
            {position}
            {duration}
            {rate}
            {isPlaying}
            on:seek={onSeek}
            on:volumeChange={onVolumeChange}
            on:rateChange={onRateChange}
            on:togglePlay={onTogglePlay}
          />
        </div>

        <!-- Chapters -->
        <ChapterList
          {chapters}
          currentPosition={position}
          onChapterSelect={(startPosition) => {
            if (audio) seekTo(startPosition)
          }}
        />
      {/if}
    </div>
  </ScrollRegion>
</ReaderLayout>

<style>
  .wa-player-audio {
    display: none;
  }

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
    text-align: center;
  }

  .header-row {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: center;
    gap: var(--wa-space-s);
  }

  .title {
    margin: 0;
    font-size: 1.25rem;
  }

  .contributors {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: 0;
    padding: 0;
    list-style: none;
    text-align: left;
  }

  .contributor {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xs);
  }

  .contributor span {
    flex: 1 1;
  }

  .custom-audio-player {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-s);
  }

  .cover {
    width: 15rem;
    margin: 0 auto;
  }


</style>
