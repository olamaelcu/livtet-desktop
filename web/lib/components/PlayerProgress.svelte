<script lang="ts">
import { createEventDispatcher } from 'svelte'

const dispatch = createEventDispatcher()

export let position: number = 0
export let duration: number = 0
export let formatTime: (seconds: number) => string

$: percent = duration > 0 ? (position / duration) * 100 : 0

function onInput(event: Event): void {
  const target = event.currentTarget as HTMLElement & { value: number }
  const newPercent = target.value
  const newPosition = (newPercent / 100) * duration
  dispatch('seek', { position: newPosition })
}
</script>

<div class="player-progress">
  <div class="times">
    <span class="current-time">{formatTime(position)}</span>
    <span class="duration-time">{formatTime(duration)}</span>
  </div>

  <wa-slider
    class="progress-slider"
    min="0"
    max="100"
    step="0.1"
    value={percent}
    oninput={onInput}
  ></wa-slider>
</div>

<style>
  .player-progress {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-xxs);
  }

  .times {
    display: flex;
    justify-content: space-between;
    color: var(--wa-color-text-quiet);
    font-size: var(--wa-font-size-xs);
    margin: var(--wa-space-xs);
  }

  .progress-slider {
    --wa-slider-track-height: var(--wa-border-width-xxs);
    --wa-slider-track-border-radius: var(--wa-border-radius-s);
    --wa-slider-track-background-color: var(--wa-color-neutral-30);
    --wa-slider-fill-color: var(--wa-color-brand-60);
    --wa-slider-handle-size: var(--wa-space-m);
    --wa-slider-handle-border-radius: var(--wa-border-radius-circle);
    --wa-slider-handle-box-shadow: var(--wa-shadow-xs);
    --wa-slider-handle-color: var(--wa-color-brand-60);
    --wa-slider-tooltip-background-color: var(--wa-color-brand-60);
    --wa-slider-tooltip-color: var(--wa-color-brand-on);
    --wa-slider-marker-color: var(--wa-color-divider);
  }
</style>
