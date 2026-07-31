// Public surface of the command system. Routes and components import
// from here, not from individual files. Keeps the dependency graph
// from leaking internal layout.

export { defaultBindings, defaultCommands } from './defaults'
export { globalCommands } from './definitions/global'
export { searchCommands } from './definitions/search'
export { helpState, paletteState } from './dialog-state.svelte'
export {
  type HotkeyBridge,
  initHotkeyBridge,
} from './hotkey-bridge.svelte'
export { useCommandRegistry } from './registry.svelte'
export { useScopeRegistry } from './scope.svelte'
export {
  loadCustomProfile,
  saveCustomProfile,
} from './storage'
export {
  asCommandId,
  type Binding,
  type Command,
  type CommandId,
  type CommandScope,
  type ConflictInfo,
  type Profile,
} from './types'
