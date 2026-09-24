import type { Hotkey } from '@tanstack/svelte-hotkeys'
import { COMMAND_BY_ID, type CommandId } from './commands'

const STORE_FILE = 'hotkeys.json'
const STORE_KEY = 'bindings'

/** User overrides layered over the defaults in {@link COMMANDS}. */
let overrides = $state<Partial<Record<CommandId, Hotkey>>>({})

export function bindingFor(id: CommandId): Hotkey {
  return overrides[id] ?? COMMAND_BY_ID[id].defaultHotkey
}

export function hasOverride(id: CommandId): boolean {
  return overrides[id] !== undefined
}

export function setBinding(id: CommandId, hotkey: Hotkey) {
  overrides = { ...overrides, [id]: hotkey }
  void persist()
}

export function clearBinding(id: CommandId) {
  const next = { ...overrides }
  delete next[id]
  overrides = next
  void persist()
}

export function resetBindings() {
  overrides = {}
  void persist()
}

async function openStore() {
  const { load } = await import('@tauri-apps/plugin-store')
  return load(STORE_FILE, { autoSave: true })
}

async function persist() {
  try {
    const store = await openStore()
    await store.set(STORE_KEY, overrides)
    await store.save()
  } catch {
    // Outside Tauri (browser preview, tests) there is nowhere to persist to.
  }
}

export async function initBindings() {
  try {
    const store = await openStore()
    const saved = await store.get<Partial<Record<CommandId, Hotkey>>>(STORE_KEY)
    if (saved) overrides = saved
  } catch {
    // Defaults remain in effect.
  }
}
