// localStorage persistence for the user's custom binding overrides.
// SSR-safe: returns an empty profile when window is undefined.
// Failures during read/write are swallowed so a corrupt or quota-full
// localStorage never breaks the bridge.

import type { Binding, CommandId } from './types'

const STORAGE_KEY = 'livtet:commands:profile:custom'

export function loadCustomProfile(): Record<CommandId, Binding> {
  if (typeof window === 'undefined') return {}
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<CommandId, Binding>
    return parsed ?? {}
  } catch {
    return {}
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null

/**
 * Save with a 500ms debounce so a flurry of recorder presses doesn't
 * hammer localStorage. Synchronous `setItem` happens inside the timer.
 */
export function saveCustomProfile(profile: Record<CommandId, Binding>): void {
  if (typeof window === 'undefined') return
  if (saveTimer) clearTimeout(saveTimer)
  saveTimer = setTimeout(() => {
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(profile))
    } catch {
      // Quota exceeded, private mode, or storage disabled. Silently drop
      // rather than throwing — the in-memory profile still works for
      // the current session.
    }
  }, 500)
}
