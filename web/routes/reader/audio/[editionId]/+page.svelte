<script lang="ts">
import { createQuery } from '@tanstack/svelte-query'
import { onMount } from 'svelte'
import { toast } from 'svelte-sonner'
import { page } from '$app/state'
import AudioBookmarks from '$lib/components/AudioBookmarks.svelte'
import AudioPlayer from '$lib/components/AudioPlayer.svelte'
import ChapterList from '$lib/components/ChapterList.svelte'
import PlayerProgress from '$lib/components/PlayerProgress.svelte'
import ScrollRegion from '$lib/layout/ScrollRegion.svelte'
import { readerKeys } from '$lib/query/keys'
import { formatTimestamp, loadReaderPublication } from '$lib/reader'
import type { Bookmark } from '$lib/reader/bookmark'
import {
  addBookmark,
  isBookmarked,
  loadBookmarks,
  removeBookmark,
  withBookmark,
  withoutBookmark,
} from '$lib/reader/bookmark'
import { clampSeek, needsPersist, resumePoint, SAVE_EVERY_SECONDS } from '$lib/reader/playback'
import { loadProgress, saveProgress } from '$lib/reader/progress'
import { coverUrlFor, loadEditionCovers, loadEditionDetail } from '$lib/search'
import ReaderLayout from '../../ReaderLayout.svelte'

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
let metadataReady = $state(false)
let savedPosition = $state<number | null>(null)
let savedReady = $state(false)

// Bookmarks on this recording; a bookmark holds no text, only a playback spot.
let bookmarks = $state<Bookmark[]>([])
let bookmarksLoaded = $state(false)

/** The spot on screen, to the second — the unit an audiobook bookmark stores. */
const currentSecond = $derived(Math.round(position))

/** Whether a bookmark save is in flight, so a second click cannot duplicate it. */
let bookmarking = $state(false)

/** Whether the current spot is already bookmarked (within a second). */
const bookmarked = $derived(
  isBookmarked(bookmarks, { kind: 'timestamp', positionSeconds: currentSecond }),
)

async function persist(force = false): Promise<void> {
  if (!editionId) return
  if (!needsPersist(position, lastSaved, force)) return
  const saved = await saveProgress(editionId, {
    kind: 'audio',
    audio: { positionSeconds: position },
  })
  if (saved) lastSaved = position
  else toast.error('Could not save listening progress')
}

function onTimeUpdate(): void {
  if (!audio) return
  position = audio.currentTime
  if (position - lastSaved >= SAVE_EVERY_SECONDS) void persist()
}

function onLoadedMetadata(): void {
  if (!audio) return
  metadataReady = true
}

/// Read the saved position back from SQLite. Resolved alongside metadata
/// loading; the resume effect below applies whichever arrives last so a fast
/// `loadedmetadata` event cannot race past the database read.
async function loadSaved(): Promise<void> {
  const progress = await loadProgress(editionId)
  savedPosition = progress?.kind === 'audio' ? (progress.audio?.positionSeconds ?? 0) : 0
  savedReady = true
}

$effect(() => {
  if (!metadataReady || !savedReady || resumed || !audio) return
  resumed = true
  const resume = resumePoint(savedPosition, audio.duration || duration)
  if (resume !== undefined) {
    audio.currentTime = resume
    position = resume
  }
  lastSaved = position
})

onMount(() => {
  void loadSaved()
  void (async () => {
    bookmarks = await loadBookmarks(editionId)
    bookmarksLoaded = true
  })()
})

/**
 * Bookmark the current playback spot.
 *
 * Only ever adds: a bookmarked spot has its button disabled, and the list owns
 * removal.
 */
async function bookmarkCurrentSpot(): Promise<void> {
  if (!bookmarksLoaded || bookmarking) return
  if (isBookmarked(bookmarks, { kind: 'timestamp', positionSeconds: currentSecond })) return
  bookmarking = true
  try {
    const added = await addBookmark(editionId, {
      kind: 'timestamp',
      positionSeconds: currentSecond,
    })
    bookmarks = withBookmark(bookmarks, added)
  } finally {
    bookmarking = false
  }
}

/** Drop a bookmark without moving playback. */
async function removeBookmarkEntry(bookmark: Bookmark): Promise<void> {
  if (!(await removeBookmark(bookmark.id))) return
  bookmarks = withoutBookmark(bookmarks, bookmark.id)
}

function seekTo(seconds: number): void {
  if (!audio) return
  const newTime = clampSeek(seconds, audio.duration || duration)
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

        <!-- Bookmarks: playback spots, listed apart from listening position -->
        <AudioBookmarks
          {bookmarks}
          canAdd={bookmarksLoaded && !bookmarking}
          {bookmarked}
          onadd={() => void bookmarkCurrentSpot()}
          onvisit={(bookmark) => seekTo(bookmark.positionSeconds ?? 0)}
          onremove={(bookmark) => void removeBookmarkEntry(bookmark)}
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
