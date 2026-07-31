<script lang="ts">
import { formatForDisplay, getHotkeyRegistrations } from '@tanstack/svelte-hotkeys'
import { defaultBindings } from '../defaults'
import { paletteState } from '../dialog-state.svelte'
import type { HotkeyBridge } from '../hotkey-bridge.svelte'
import { useCommandRegistry } from '../registry.svelte'
import type { Command, CommandId } from '../types'
import KeyRecorder from './key-recorder.svelte'

interface Props {
  bridge: HotkeyBridge
}

let { bridge }: Props = $props()

const registry = useCommandRegistry()
const registrations = getHotkeyRegistrations()

let query = $state('')
let focusedId: CommandId | null = $state(null)

const visible = $derived.by(() => {
  const q = query.trim().toLowerCase()
  const all = registry.all()
  if (q === '') return all
  return all.filter(
    (c) =>
      c.label.toLowerCase().includes(q) ||
      c.id.toLowerCase().includes(q) ||
      (c.description ?? '').toLowerCase().includes(q),
  )
})

const grouped = $derived.by(() => {
  const out = new Map<string, Command[]>()
  for (const c of visible) {
    const list = out.get(c.category) ?? []
    list.push(c)
    out.set(c.category, list)
  }
  return Array.from(out.entries())
})

$effect(() => {
  console.log("[command-palette] focus effect run, open=", paletteState.open, "visibleLen=", visible.length, "focusedId=", focusedId);
  if (paletteState.open && visible.length > 0 && focusedId === null) {
    focusedId = visible[0]?.id ?? null
    console.log("[command-palette] focus effect SET focusedId=", focusedId);
  }
})

function runAndClose(c: Command) {
  c.run()
  paletteState.open = false
  query = ''
  focusedId = null
}

function onKeydown(event: KeyboardEvent) {
  if (!paletteState.open) return
  if (event.key === 'Escape') {
    event.preventDefault()
    paletteState.open = false
    query = ''
    focusedId = null
    return
  }
  if (event.key === 'ArrowDown') {
    event.preventDefault()
    const idx = visible.findIndex((c) => c.id === focusedId)
    const next = visible[idx + 1] ?? visible[0]
    if (next) focusedId = next.id
    return
  }
  if (event.key === 'ArrowUp') {
    event.preventDefault()
    const idx = visible.findIndex((c) => c.id === focusedId)
    const prev = idx <= 0 ? visible[visible.length - 1] : visible[idx - 1]
    if (prev) focusedId = prev.id
    return
  }
  if (event.key === 'Enter') {
    event.preventDefault()
    const target = visible.find((c) => c.id === focusedId)
    if (target) runAndClose(target)
    return
  }
}

function bindingFor(c: Command): string {
  const reg = registrations.hotkeys.find((r) => r.id === c.id)
  if (reg) return reg.hotkey
  const seq = registrations.sequences.find((r) => r.id === c.id)
  if (seq) return seq.sequence.join(' ')
  const fallback = defaultBindings[c.id]
  if (Array.isArray(fallback)) return fallback.join(' ')
  return fallback ?? ''
}

function recorderBindingFor(c: Command) {
  const resolved = bridge.resolved[c.id]
  if (Array.isArray(resolved)) return 'Mod+?'
  return (resolved as string | undefined) ?? (defaultBindings[c.id] as string) ?? ''
}
</script>

<svelte:window onkeydown={onKeydown} />

<wa-dialog
  open={paletteState.open}
  label="Command palette"
  light-dismiss
  onwa-after-hide={() => {
    paletteState.open = false;
    query = "";
    focusedId = null;
  }}
>
  <input
    type="search"
    placeholder="Type a command…"
    bind:value={query}
    autofocus
  />

  <div class="results">
    {#each grouped as [category, cmds] (category)}
      <section>
        <h3>{category}</h3>
        <ul>
          {#each cmds as c (c.id)}
            <li class:focused={c.id === focusedId}>
              <button
                type="button"
                class="row"
                onclick={() => runAndClose(c)}
                onmouseenter={() => (focusedId = c.id)}
              >
                <span class="label">{c.label}</span>
                <span class="binding">
                  {formatForDisplay(bindingFor(c))}
                </span>
              </button>
              <KeyRecorder
                commandId={c.id}
                currentBinding={recorderBindingFor(c) as never}
                onSave={(id, b) => bridge.saveBinding(id, b as never)}
              />
            </li>
          {/each}
        </ul>
      </section>
    {/each}

    {#if visible.length === 0}
      <p class="empty">No commands match "{query}".</p>
    {/if}
  </div>
</wa-dialog>

<style>
  wa-dialog::part(panel) {
    width: min(40rem, 90vw);
    max-height: 70vh;
  }

  input[type="search"] {
    width: 100%;
    padding: 0.5rem 0.75rem;
    font-size: 1rem;
    border: 1px solid var(--wa-color-surface-border, rgba(0, 0, 0, 0.1));
    border-radius: 6px;
    margin-bottom: 0.5rem;
    box-sizing: border-box;
  }

  .results {
    max-height: 50vh;
    overflow-y: auto;
  }

  h3 {
    margin: 0.75rem 0 0.25rem;
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--wa-color-text-quiet, currentColor);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
  }

  li.focused {
    background: var(--wa-color-brand-fill-quiet, rgba(0, 112, 243, 0.08));
  }

  .row {
    flex: 1;
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: transparent;
    border: 0;
    padding: 0.25rem 0.5rem;
    font-size: 0.9375rem;
    text-align: left;
    cursor: pointer;
  }

  .binding {
    font-family: var(--wa-font-family-code, monospace);
    font-size: 0.75rem;
    color: var(--wa-color-text-quiet, currentColor);
  }

  .empty {
    padding: 1rem;
    text-align: center;
    color: var(--wa-color-text-quiet, currentColor);
  }
</style>