import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { AtprotoLoginStart, AtprotoProfileView, AtprotoStatus } from './bindings'

// Strict re-export of the generated contract — single source of truth.
export type { AtprotoLoginStart, AtprotoProfileView, AtprotoStatus }

export async function beginAtprotoLogin(handle: string): Promise<AtprotoLoginStart> {
  return invoke<AtprotoLoginStart>('atproto_begin_login', { handle })
}

export async function completeAtprotoLogin(callbackUrl: string): Promise<AtprotoStatus> {
  return invoke<AtprotoStatus>('atproto_complete_login', { callbackUrl })
}

export async function atprotoStatus(): Promise<AtprotoStatus | null> {
  return invoke<AtprotoStatus | null>('atproto_status')
}

export async function atprotoLogout(): Promise<void> {
  return invoke<void>('atproto_logout')
}

export async function updateAtprotoProfile(input: {
  displayName: string
  description: string
}): Promise<AtprotoProfileView> {
  return invoke<AtprotoProfileView>('atproto_update_profile', input)
}

export async function setAtprotoAvatar(avatarPath: string): Promise<AtprotoProfileView> {
  return invoke<AtprotoProfileView>('atproto_set_avatar', { avatarPath })
}

export async function atprotoGetProfile(): Promise<AtprotoProfileView | null> {
  return invoke<AtprotoProfileView | null>('atproto_get_profile')
}

export function atprotoErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object') {
    const value = Object.values(error as Record<string, unknown>)[0]
    if (typeof value === 'string') return value
    if (value && typeof value === 'object' && 'message' in value) {
      return String(value.message)
    }
  }
  return 'ATProto request failed'
}

/**
 * Subscribe to the results of the deep-link callback flow, which finishes
 * outside any command's lifetime and reports through events instead.
 */
export async function listenToAtprotoEvents(handlers: {
  onLogin: (status: AtprotoStatus) => void
  onError: (message: string) => void
}): Promise<UnlistenFn> {
  const unlistenLogin = await listen<AtprotoStatus>('atproto://login', (event) =>
    handlers.onLogin(event.payload),
  )
  const unlistenError = await listen<string>('atproto://error', (event) =>
    handlers.onError(event.payload),
  )
  return () => {
    unlistenLogin()
    unlistenError()
  }
}
