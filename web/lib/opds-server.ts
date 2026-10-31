import { invoke } from '@tauri-apps/api/core'
import type { OpdsServerError, OpdsServerSettings, OpdsServerStatus } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { OpdsServerError, OpdsServerSettings, OpdsServerStatus }

/** The stored OPDS server settings plus whether a listener is bound right now. */
export async function getOpdsServerStatus(): Promise<OpdsServerStatus> {
  return invoke<OpdsServerStatus>('opds_server_status')
}

/**
 * Persist the OPDS server settings and reconcile the listener with them.
 * Idempotent: saving the configuration already in force leaves the bound
 * socket, and any connected e-reader, alone.
 */
export async function saveOpdsServerSettings(
  settings: OpdsServerSettings,
): Promise<OpdsServerStatus> {
  return invoke<OpdsServerStatus>('opds_server_settings_save', { settings })
}

/**
 * Unwraps an `OpdsServerError` (a `{ code, message }` struct) or a plain
 * string into a human-readable message. Mirrors `mcpErrorMessage`.
 */
export function opdsServerErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error
    if (typeof message === 'string' && message) return message
  }
  return 'OPDS server settings request failed'
}
