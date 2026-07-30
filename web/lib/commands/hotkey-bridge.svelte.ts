// The reactive bridge between our command registry and the TanStack
// Hotkey adapter. Watches three reactive sources (registry, scopes,
// custom profile) and exposes a saveBinding function. The actual hotkey
// mounting happens in components/command-reconciler.svelte, which reads
// the bridge's derived state and calls createHotkey/createHotkeySequence
// via {#each} rows that auto-unregister on unmount.

import {
  type HotkeyCallback,
} from "@tanstack/svelte-hotkeys";

import { useCommandRegistry } from "./registry.svelte";
import { useScopeRegistry } from "./scope.svelte";
import { defaultBindings } from "./defaults";
import { loadCustomProfile, saveCustomProfile } from "./storage";
import type {
  Binding,
  Command,
  CommandId,
  CommandScope,
} from "./types";

export type ActiveRegistration = {
  readonly id: CommandId;
  readonly binding: Binding;
  readonly command: Command;
};

export function resolvedBinding(
  id: CommandId,
  custom: Readonly<Record<CommandId, Binding>>,
): Binding | null {
  return custom[id] ?? defaultBindings[id] ?? null;
}

export function isActive(
  command: Command,
  activeScopes: ReadonlySet<CommandScope>,
): boolean {
  const inScope =
    command.scope === "global" || activeScopes.has(command.scope);
  if (!inScope) return false;
  if (command.when && !command.when()) return false;
  return true;
}

/**
 * Compute the desired registrations for the current state. Pure, so the
 * reconciler can call it freely and compare references for change detection.
 */
export function deriveActive(
  commands: readonly Command[],
  activeScopes: ReadonlySet<CommandScope>,
  custom: Readonly<Record<CommandId, Binding>>,
): readonly ActiveRegistration[] {
  const out: ActiveRegistration[] = [];
  for (const c of commands) {
    if (!isActive(c, activeScopes)) continue;
    const binding = resolvedBinding(c.id, custom);
    if (binding === null) continue;
    out.push({ id: c.id, binding, command: c });
  }
  return out;
}

/**
 * Initialize the bridge. Must be called once from a component that lives
 * for the lifetime of the app (the root layout).
 *
 * Returns:
 *   - `resolvedBindings`: a reactive `Record<CommandId, Binding>`
 *     for components that need to render the user's current binding for
 *     a command (palette, cheatsheet, recorder).
 *   - `conflictSet`: a reactive `Set<string>` of normalized bindings
 *     that have two or more active commands.
 *   - `saveBinding(commandId, newBinding)`: persist a custom override.
 *   - `getCustomProfile()`: snapshot of the current custom profile.
 */
export function initHotkeyBridge() {
  const registry = useCommandRegistry();
  const scopes = useScopeRegistry();

  // Custom profile is the only piece of state the bridge owns directly;
  // we mutate it via `saveBinding` and the reconciler watches it via
  // the snapshot below.
  let customProfile = $state<Record<CommandId, Binding>>(loadCustomProfile());

  const resolved = $derived.by(() => {
    const out: Record<CommandId, Binding> = {};
    for (const c of registry.all()) {
      const b = resolvedBinding(c.id, customProfile);
      if (b !== null) out[c.id] = b;
    }
    return out;
  });

  const conflicts = $derived.by(() => {
    const byBinding = new Map<string, CommandId[]>();
    for (const c of registry.all()) {
      if (!isActive(c, scopes.active)) continue;
      const b = resolvedBinding(c.id, customProfile);
      if (b === null) continue;
      const key = JSON.stringify(b);
      const list = byBinding.get(key) ?? [];
      list.push(c.id);
      byBinding.set(key, list);
    }
    const conflictKeys = new Set<string>();
    for (const [key, ids] of byBinding) {
      if (ids.length > 1) conflictKeys.add(key);
    }
    return conflictKeys;
  });

  function saveBinding(id: CommandId, binding: Binding): void {
    customProfile = { ...customProfile, [id]: binding };
    saveCustomProfile(customProfile);
  }

  return {
    resolved,
    conflicts,
    saveBinding,
    getCustomProfile: (): Readonly<Record<CommandId, Binding>> => customProfile,
  };
}

export type HotkeyBridge = ReturnType<typeof initHotkeyBridge>;

/** Build a callback that runs the command's run function. */
export function buildCallback(command: Command): HotkeyCallback {
  return (_event, _ctx) => {
    command.run(_event);
  };
}