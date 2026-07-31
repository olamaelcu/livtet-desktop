<script lang="ts">
import { createHotkeyRecorder, formatForDisplay, type Hotkey } from '@tanstack/svelte-hotkeys'
import type { CommandId } from '../types'

interface Props {
  commandId: CommandId
  currentBinding: Hotkey
  onSave: (commandId: CommandId, newBinding: Hotkey) => void
}

let { commandId, currentBinding, onSave }: Props = $props()

const recorder = createHotkeyRecorder({
  onRecord: (hotkey) => {
    onSave(commandId, hotkey)
  },
})

function start() {
  recorder.startRecording()
}
</script>

<button
  type="button"
  class="recorder-button"
  class:recording={recorder.isRecording}
  onclick={start}
>
  {recorder.isRecording
    ? "Press keys…"
    : recorder.recordedHotkey
    ? formatForDisplay(recorder.recordedHotkey)
    : formatForDisplay(currentBinding)}
</button>

{#if recorder.isRecording}
  <small class="hint">Esc to cancel · Backspace to clear</small>
{/if}

<style>
  .recorder-button {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.75rem;
    padding: 0.25rem 0.625rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    background: var(--wa-color-surface-default, white);
    border-radius: 4px;
    cursor: pointer;
  }

  .recorder-button.recording {
    border-color: var(--wa-color-brand-fill-loud, #0070f3);
    background: var(--wa-color-brand-fill-quiet, rgba(0, 112, 243, 0.08));
  }

  .hint {
    display: block;
    font-size: 0.6875rem;
    color: var(--wa-color-text-quiet, currentColor);
    margin-top: 0.25rem;
  }
</style>
