import { invoke } from '@tauri-apps/api/core'
import type { PluginSummary, RemotePlugin } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { PluginSummary, RemotePlugin }

export async function loadPlugins(): Promise<PluginSummary[]> {
  return invoke<PluginSummary[]>('list_plugins')
}

export async function addPluginFromPath(source: string): Promise<PluginSummary> {
  return invoke<PluginSummary>('add_plugin_from_path', { source })
}

export async function removePlugin(name: string): Promise<void> {
  return invoke<void>('remove_plugin', { name })
}

/**
 * Turns a plugin on or off by moving its directory between the scanned plugins
 * root and the disabled one (ADR-0034). Idempotent, so re-asserting the state a
 * plugin is already in resolves without doing anything.
 */
export async function setPluginEnabled(name: string, enabled: boolean): Promise<void> {
  return invoke<void>('set_plugin_enabled', { name, enabled })
}

export async function discoverRemotePlugins(registryUrl: string): Promise<RemotePlugin[]> {
  return invoke<RemotePlugin[]>('discover_remote_plugins', { registryUrl })
}

export async function installRemotePlugin(registryUrl: string, id: string): Promise<PluginSummary> {
  return invoke<PluginSummary>('install_remote_plugin', { registryUrl, id })
}

/**
 * Unwraps a `PluginError` (a tagged enum serialized as `{ Variant: { message } }`)
 * or a plain string into a human-readable message. Mirrors `opdsErrorMessage`.
 */
export function pluginErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object') {
    const value = Object.values(error as Record<string, unknown>)[0]
    if (typeof value === 'string') return value
    if (value && typeof value === 'object' && 'message' in value) {
      return String((value as { message: unknown }).message)
    }
  }
  return 'Plugin request failed'
}
