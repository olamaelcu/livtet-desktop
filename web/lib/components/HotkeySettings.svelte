<script lang="ts">
import { resetBindings } from '../hotkeys/bindings.svelte'
import { COMMANDS } from '../hotkeys/commands'
import ActionButton from './ActionButton.svelte'
import HotkeyRow from './HotkeyRow.svelte'
</script>

<section class="hotkeys" aria-labelledby="hotkeys-heading">
  <div class="hotkeys-header">
    <h2 id="hotkeys-heading">Keyboard</h2>
    <ActionButton onclick={resetBindings}>Reset all</ActionButton>
  </div>
  {#each COMMANDS as command (command.id)}
    <HotkeyRow {command} />
  {/each}
</section>

<style>
  .hotkeys h2 {
    margin: 0;
  }

  .hotkeys-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
    margin-bottom: var(--wa-space-l);
  }
</style>
