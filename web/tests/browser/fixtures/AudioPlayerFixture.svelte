<script lang="ts">
import AudioPlayer from '$lib/components/AudioPlayer.svelte'

interface Props {
  position?: number
  duration?: number
  rate?: number
  isPlaying?: boolean
  /** Stands in for the media element; the player only reads and writes these. */
  audio?: { duration: number; currentTime: number; volume: number; playbackRate: number } | null
}

let { position = 0, duration = 300, rate = 1, isPlaying = false, audio = null }: Props = $props()

// The player reports through createEventDispatcher, so a parent is needed to
// observe the events. Each is written into the DOM for the test to read.
let seeked = $state<number | null>(null)
let rateChanged = $state<number | null>(null)
let volumeChanged = $state<number | null>(null)
let toggles = $state(0)
</script>

<div
  data-seeked={seeked === null ? '' : String(seeked)}
  data-rate={rateChanged === null ? '' : String(rateChanged)}
  data-volume={volumeChanged === null ? '' : String(volumeChanged)}
  data-toggles={String(toggles)}
>
  <AudioPlayer
    {audio}
    {position}
    {duration}
    {rate}
    {isPlaying}
    on:seek={(event) => (seeked = event.detail.position)}
    on:rateChange={(event) => (rateChanged = event.detail.rate)}
    on:volumeChange={(event) => (volumeChanged = event.detail.volume)}
    on:togglePlay={() => (toggles += 1)}
  />
</div>
