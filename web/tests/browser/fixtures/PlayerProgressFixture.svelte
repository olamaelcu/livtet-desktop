<script lang="ts">
import PlayerProgress from '$lib/components/PlayerProgress.svelte'

interface Props {
  position?: number
  duration?: number
}

let { position = 0, duration = 0 }: Props = $props()

// PlayerProgress reports seeks through createEventDispatcher, so a parent is
// needed to observe one. The received position is written into the DOM for the
// test to read, rather than into a spy.
let seeked = $state<number | null>(null)

function formatTime(seconds: number): string {
  const whole = Math.floor(seconds)
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, '0')}`
}
</script>

<div data-seeked={seeked === null ? '' : String(seeked)}>
  <PlayerProgress
    {position}
    {duration}
    {formatTime}
    on:seek={(event) => (seeked = event.detail.position)}
  />
</div>
