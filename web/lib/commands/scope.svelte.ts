// Tracks which non-global scopes are currently active. The bridge reads
// from this to decide which commands to register; the <CommandScope>
// component flips membership on mount/unmount.
//
// Module-state singleton is fine here — there's only ever one app, and
// HMR during dev tears down and re-creates the module so the Set resets.

import type { CommandScope } from './types'

class ScopeRegistry {
  active = $state<Set<CommandScope>>(new Set(['global']))

  /** Idempotent. Calling twice with the same id is a no-op. */
  activate(scope: CommandScope): void {
    if (this.active.has(scope)) return
    this.active = new Set([...this.active, scope])
  }

  /** Idempotent. Always retains `"global"`. */
  deactivate(scope: CommandScope): void {
    if (scope === 'global') return
    if (!this.active.has(scope)) return
    const next = new Set(this.active)
    next.delete(scope)
    this.active = next
  }

  isActive(scope: CommandScope): boolean {
    return this.active.has(scope)
  }
}

const registry = new ScopeRegistry()

/** Get the singleton. Components and the bridge both call this. */
export function useScopeRegistry(): ScopeRegistry {
  return registry
}
