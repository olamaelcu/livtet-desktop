// Core types for the command system. Commands and bindings are data;
// the bridge in hotkey-bridge.svelte.ts turns them into TanStack Hotkey
// registrations. This module has no runtime side effects beyond a
// single branded-id cast helper.

import type { Hotkey, HotkeySequence } from '@tanstack/svelte-hotkeys'

declare const CommandIdBrand: unique symbol
export type CommandId = string & { readonly [CommandIdBrand]: true }

/** Const-cast: a plain string becomes a CommandId at the call site. */
export const asCommandId = (s: string): CommandId => s as CommandId

/**
 * Scope of a command. `"global"` is always active. Anything else is a
 * named scope activated by a <CommandScope> wrapper.
 */
export type CommandScope = 'global' | (string & {})

/**
 * A discrete action the user can invoke via a hotkey, palette row, or
 * future surface (right-click menu, command bar, etc.).
 */
export interface Command {
  readonly id: CommandId
  readonly label: string
  readonly description?: string
  readonly category: string
  readonly icon?: string
  readonly scope: CommandScope
  /**
   * Dynamic predicate. When it returns false, the command is registered
   * with the adapter (visible in devtools) but the callback does not
   * run. The bridge re-evaluates this every time it re-derives the
   * active set.
   */
  readonly when?: () => boolean
  readonly run: (event?: KeyboardEvent) => void
}

/** A single binding: a chord (`Mod+S`) or a sequence (`['G', 'G']`). */
export type Binding = Hotkey | HotkeySequence

/** Map of command id → binding. Overrides live in localStorage. */
export type Profile = Readonly<Record<CommandId, Binding>>

/** Two+ commands sharing the same binding. Surfaced in the palette. */
export interface ConflictInfo {
  readonly hotkey: string
  readonly commandIds: readonly CommandId[]
}
