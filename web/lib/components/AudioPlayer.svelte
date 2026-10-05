<script lang="ts">
import { createEventDispatcher } from 'svelte'
import ActionButton from './ActionButton.svelte'

const dispatch = createEventDispatcher()

export const SKIP_SECONDS: number = 30
export const RATES: number[] = [1, 1.25, 1.5, 1.75, 2]

let { audio, position, duration, rate, isPlaying } = $props()

function seekTo(seconds: number): void {
  if (audio) {
    const max = audio.duration || duration
    const newTime = Math.min(Math.max(0, seconds), max)
    audio.currentTime = newTime
    dispatch('seek', { position: newTime })
  }
}

function chooseRate(event: Event): void {
  const value = Number((event.target as HTMLSelectElement).value)
  if (RATES.includes(value)) {
    dispatch('rateChange', { rate: value })
    if (audio) audio.playbackRate = value
  }
}

function togglePlay(): void {
  isPlaying = !isPlaying
  dispatch('togglePlay')
}

function handleVolume(event: Event): void {
  const input = event.target as HTMLInputElement
  const value = Number(input.value)
  const percent = value / 100
  if (audio) {
    audio.volume = percent
  }
  dispatch('volumeChange', { volume: percent })
}
</script>

<!-- Controls section -->
<div class="wa-player-controls">
  <wa-button onclick={() => seekTo(position - SKIP_SECONDS)}>
    <wa-icon slot="start" name="backward"></wa-icon>
    {SKIP_SECONDS}s
  </wa-button>

  <wa-button onclick={togglePlay}>
    <wa-icon name={isPlaying ? "pause" : "play"}></wa-icon>
  </wa-button>

  <wa-button onclick={() => seekTo(position + SKIP_SECONDS)}>
    {SKIP_SECONDS}s
    <wa-icon slot="end" name="forward"></wa-icon>
  </wa-button>

  <wa-select
    aria-label="Playback speed"
    value={String(rate)}
    onchange={chooseRate}
  >
    {#each RATES as option (option)}
      <wa-option value={String(option)}>{option}×</wa-option>
    {/each}
  </wa-select>
</div>

<!-- Volume Slider -->
<div class="wa-player-volume">
  <wa-icon name="volume-high" class="wa-volume-icon"></wa-icon>
  <input
    type="range"
    min="0"
    max="100"
    value={Math.round((audio?.volume ?? 1) * 100)}
    oninput={handleVolume}
  />
</div>

<style>
  .wa-player-controls {
    display: flex;
    align-items: center;
    gap: var(--wa-space-s);
    margin: var(--wa-space-s) 0;
  }

  .wa-player-volume {
    display: flex;
    align-items: center;
    gap: var(--wa-space-xxs);
  }

  .wa-volume-icon {
    color: var(--wa-color-text-quiet);
  }

  input[type="range"] {
    width: 8rem;
    accent-color: var(--wa-color-brand-60);
  }

  .play-pause-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 4rem;
    height: 4rem;
    border-radius: var(--wa-border-radius-s);
    background: var(--wa-color-neutral-95);
    color: var(--wa-color-brand-60);
    border: 1px solid var(--wa-color-neutral-30);
    cursor: pointer;
    user-select: none;
  }

  .play-pause-btn i {
    width: 2rem;
    height: 2rem;
    font-size: 2rem;
    color: var(--wa-color-brand-60);
  }

  .s {
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-quiet);
  }
</style>
