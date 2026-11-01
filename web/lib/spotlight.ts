import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { SpotlightError, SpotlightStatus } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { SpotlightError, SpotlightStatus }

/** Event asking the web layer to open one edition after a Spotlight selection. */
export const SPOTLIGHT_OPEN_EDITION_EVENT = 'spotlight://open-edition'

export async function getSpotlightStatus(): Promise<SpotlightStatus> {
  return invoke<SpotlightStatus>('spotlight_status')
}

/** Opt in (writing the library) or out (deleting the Spotlight domain). */
export async function setSpotlightEnabled(enabled: boolean): Promise<SpotlightStatus> {
  return invoke<SpotlightStatus>('spotlight_set_enabled', { enabled })
}

/** Rewrite the whole index from the current library. */
export async function reindexSpotlight(): Promise<SpotlightStatus> {
  return invoke<SpotlightStatus>('spotlight_reindex')
}

/**
 * Unwraps a `SpotlightError` (a `{ code, message }` struct) or a plain string
 * into a human-readable message. Mirrors `opdsServerErrorMessage`.
 */
export function spotlightErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error
    if (typeof message === 'string' && message) return message
  }
  return 'Spotlight request failed'
}

/** Fires when a Spotlight result is chosen; the payload is the edition id. */
export function listenToSpotlightSelections(
  onEdition: (editionId: string) => void,
): Promise<UnlistenFn> {
  return listen<string>(SPOTLIGHT_OPEN_EDITION_EVENT, (event) => onEdition(event.payload))
}
