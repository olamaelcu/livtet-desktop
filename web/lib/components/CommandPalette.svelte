<script lang="ts">
import { formatForDisplay } from '@tanstack/svelte-hotkeys'
import { bindingFor } from '../hotkeys/bindings.svelte'
import { COMMANDS, type CommandId } from '../hotkeys/commands'

interface Props {
  onrun: (id: CommandId) => void
  onclose: () => void
}

let { onrun, onclose }: Props = $props()

let filter = $state('')
let selected = $state(0)

const results = $derived(
  COMMANDS.filter((command) => command.label.toLowerCase().includes(filter.trim().toLowerCase())),
)
const active = $derived(results.length === 0 ? -1 : Math.min(selected, results.length - 1))

function move(delta: number) {
  if (results.length === 0) return
  selected = (active + delta + results.length) % results.length
}

function run(commandId: CommandId) {
  onrun(commandId)
  onclose()
}

function handleInput(event: Event) {
  filter = (event.target as HTMLInputElement).value
  selected = 0
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    move(1)
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    move(-1)
  } else if (event.key === 'Enter') {
    event.preventDefault()
    const command = results[active]
    if (command) run(command.id)
  } else if (event.key === 'Escape') {
    onclose()
  }
}
</script>

<wa-dialog open label="Command palette" onwa-after-hide={onclose}>
  <!--
    `wa-input` is a WebAwesome custom element: its interactive semantics live in
    its shadow DOM, which Svelte's static a11y checks cannot see. A command
    palette is expected to grab focus on open, so suppress these rules.
  -->
  <!-- svelte-ignore a11y_autofocus -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <wa-input
    autofocus
    placeholder="Type a command…"
    value={filter}
    oninput={handleInput}
    onkeydown={handleKeydown}
  ></wa-input>
  <ul class="palette-list">
    {#each results as command, index (command.id)}
      <li>
        <button
          type="button"
          class="palette-item"
          class:active={index === active}
          onclick={() => run(command.id)}
        >
          <span>{command.label}</span>
          <kbd>{formatForDisplay(bindingFor(command.id))}</kbd>
        </button>
      </li>
    {:else}
      <li class="palette-empty">No matching commands.</li>
    {/each}
  </ul>
</wa-dialog>

<style>
  wa-input {
    width: 100%;
  }

  .palette-list {
    display: flex;
    flex-direction: column;
    gap: var(--wa-space-3xs);
    margin: var(--wa-space-m) 0 0;
    padding: 0;
    list-style: none;
    max-height: 18rem;
    overflow-y: auto;
  }

  .palette-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--wa-space-m);
    width: 100%;
    padding: var(--wa-space-s) var(--wa-space-m);
    border: none;
    border-radius: var(--wa-radius-m);
    background: none;
    color: var(--wa-color-text-default);
    cursor: pointer;
    text-align: left;
    font: inherit;
  }

  .palette-item:hover,
  .palette-item.active {
    background: var(--wa-color-surface-alt);
  }

  .palette-item kbd {
    font-family: var(--wa-font-family-code);
    font-size: var(--wa-font-size-xs);
    color: var(--wa-color-text-secondary);
  }

  .palette-empty {
    padding: var(--wa-space-s) var(--wa-space-m);
    color: var(--wa-color-text-secondary);
  }
</style>
