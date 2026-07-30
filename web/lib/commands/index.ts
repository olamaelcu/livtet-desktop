// Public surface of the command system. Routes and components import
// from here, not from individual files. Keeps the dependency graph
// from leaking internal layout.

export {
  asCommandId,
  type Command,
  type Binding,
  type CommandId,
  type CommandScope,
  type Profile,
  type ConflictInfo,
} from "./types";

export { useCommandRegistry } from "./registry.svelte";
export { useScopeRegistry } from "./scope.svelte";

export { defaultCommands, defaultBindings } from "./defaults";
export {
  loadCustomProfile,
  saveCustomProfile,
} from "./storage";

export {
  initHotkeyBridge,
  type HotkeyBridge,
} from "./hotkey-bridge.svelte";

export { globalCommands } from "./definitions/global";
export { searchCommands } from "./definitions/search";

export { paletteState, helpState } from "./dialog-state.svelte";