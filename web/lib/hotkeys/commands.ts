import type { Hotkey } from '@tanstack/svelte-hotkeys'

export type CommandId =
  | 'palette.open'
  | 'search.focus'
  | 'nav.library'
  | 'nav.settings'
  | 'app.refresh'

export interface CommandDefinition {
  id: CommandId
  label: string
  defaultHotkey: Hotkey
}

export const COMMANDS: readonly CommandDefinition[] = [
  { id: 'palette.open', label: 'Open command palette', defaultHotkey: 'Mod+K' },
  { id: 'search.focus', label: 'Focus search', defaultHotkey: '/' },
  { id: 'nav.library', label: 'Go to Library', defaultHotkey: 'Mod+1' },
  { id: 'nav.settings', label: 'Go to Settings', defaultHotkey: 'Mod+2' },
  { id: 'app.refresh', label: 'Refresh data', defaultHotkey: 'Mod+Shift+R' },
]

export const COMMAND_BY_ID: Record<CommandId, CommandDefinition> = Object.fromEntries(
  COMMANDS.map((command) => [command.id, command]),
) as Record<CommandId, CommandDefinition>
