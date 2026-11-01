import { invoke } from '@tauri-apps/api/core'
import type { KoreaderSyncError, KoreaderSyncSettings, KoreaderSyncStatus } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { KoreaderSyncError, KoreaderSyncSettings, KoreaderSyncStatus }

/** The stored configuration plus whether a listener is bound right now. */
export async function getKoreaderSyncStatus(): Promise<KoreaderSyncStatus> {
  return invoke<KoreaderSyncStatus>('koreader_sync_status')
}

/**
 * Persist the configuration and reconcile the listener with it.
 *
 * `password` is write-only: omit it to keep the stored one. Idempotent — saving
 * the configuration already in force leaves the bound socket alone.
 */
export async function saveKoreaderSyncSettings(
  settings: KoreaderSyncSettings,
): Promise<KoreaderSyncStatus> {
  return invoke<KoreaderSyncStatus>('koreader_sync_settings_save', { settings })
}

/** Bind the listener at the stored port, and remember that it is on. */
export async function startKoreaderSync(): Promise<KoreaderSyncStatus> {
  return invoke<KoreaderSyncStatus>('koreader_sync_start')
}

/** Unbind the listener, and remember that it is off. */
export async function stopKoreaderSync(): Promise<KoreaderSyncStatus> {
  return invoke<KoreaderSyncStatus>('koreader_sync_stop')
}

/**
 * Unwraps a `KoreaderSyncError` (a `{ code, message }` struct) or a plain
 * string into a human-readable message. Mirrors `opdsServerErrorMessage`.
 */
export function koreaderSyncErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error
    if (typeof message === 'string' && message) return message
  }
  return 'KOReader sync request failed'
}
