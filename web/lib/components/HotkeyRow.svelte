<script lang="ts">
import { createHotkeyRecorder, formatForDisplay } from '@tanstack/svelte-hotkeys'
import { bindingFor, clearBinding, hasOverride, setBinding } from '../hotkeys/bindings.svelte'
import type { CommandDefinition } from '../hotkeys/commands'
import ActionButton from './ActionButton.svelte'

interface Props {
  command: CommandDefinition
}

let { command }: Props = $props()

const recorder = createHotkeyRecorder(() => ({
  onRecord: (hotkey) => setBinding(command.id, hotkey),
}))

const binding = $derived(bindingFor(command.id))
const overridden = $derived(hasOverride(command.id))
</script>

<div class="hotkey-row">
  <span class="hotkey-label">{command.label}</span>
  <kbd class="hotkey-binding">
    {#if recorder.isRecording}
      {recorder.recordedHotkey ? formatForDisplay(recorder.recordedHotkey) : 'Press keys…'}
    {:else}
      {formatForDisplay(binding)}
    {/if}
  </kbd>
  <div class="hotkey-actions">
    {#if recorder.isRecording}
      <ActionButton onclick={() => recorder.cancelRecording()}>Cancel</ActionButton>
    {:else}
      <ActionButton onclick={() => recorder.startRecording()}>Record</ActionButton>
    {/if}
    <ActionButton disabled={!overridden} onclick={() => clearBinding(command.id)}>
      Reset
    </ActionButton>
  </div>
</div>

<style>
  .hotkey-row {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: var(--wa-space-m);
    padding: var(--wa-space-xs) 0;
    border-top: 0.0625rem solid var(--wa-color-border-default);
  }

  .hotkey-row:first-of-type {
    border-top: none;
  }

  .hotkey-binding {
    font-family: var(--wa-font-family-code);
    font-size: var(--wa-font-size-s);
    color: var(--wa-color-text-secondary);
    min-width: 6rem;
    text-align: right;
  }

  .hotkey-actions {
    display: flex;
    gap: var(--wa-space-xs);
  }
</style>
