import { invoke } from '@tauri-apps/api/core'
import type { McpSettings, McpSettingsError, McpStatus } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { McpSettings, McpSettingsError, McpStatus }

/** The stored MCP settings plus whether the endpoint is listening right now. */
export async function getMcpStatus(): Promise<McpStatus> {
  return invoke<McpStatus>('mcp_settings_get')
}

/**
 * Persist the MCP settings and reconcile the listener with them. Idempotent:
 * saving the configuration already in force leaves the bound socket, and any
 * connected client, alone.
 */
export async function saveMcpSettings(settings: McpSettings): Promise<McpStatus> {
  return invoke<McpStatus>('mcp_settings_save', { settings })
}

/**
 * Unwraps an `McpSettingsError` (a `{ code, message }` struct) or a plain
 * string into a human-readable message. Mirrors `pluginErrorMessage`.
 */
export function mcpErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { message } = error
    if (typeof message === 'string' && message) return message
  }
  return 'MCP settings request failed'
}
