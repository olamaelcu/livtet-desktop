// The reactive registry of all known commands. Pages call `register()`
// from their <script>; the bridge reads via `all()` to compute the
// active set; the palette and cheatsheet read via `all()` to render
// rows. Removing a command on unmount is the consumer's responsibility
// (component teardown handles it for our definitions/* modules).

import type { Command, CommandId } from './types'

class CommandRegistry {
  commands = $state<Map<CommandId, Command>>(new Map())

  register(command: Command): void {
    if (this.commands.has(command.id)) return
    const next = new Map(this.commands)
    next.set(command.id, command)
    this.commands = next
  }

  unregister(id: CommandId): void {
    if (!this.commands.has(id)) return
    const next = new Map(this.commands)
    next.delete(id)
    this.commands = next
  }

  get(id: CommandId): Command | undefined {
    return this.commands.get(id)
  }

  /** Snapshot of all commands in insertion order. */
  all(): readonly Command[] {
    return Array.from(this.commands.values())
  }
}

const registry = new CommandRegistry()

export function useCommandRegistry(): CommandRegistry {
  return registry
}
