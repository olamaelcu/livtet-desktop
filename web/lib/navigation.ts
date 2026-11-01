import { listen, type UnlistenFn } from '@tauri-apps/api/event'

/** Routes native chrome (tray and application menu) can open. Mirrors `crates/livtet-desktop/src/tray.rs`. */
export type AppRoute = '/library' | '/catalog' | '/plugins' | '/settings'

export const APP_NAVIGATE_EVENT = 'app://navigate'

export function listenToAppNavigation(onNavigate: (route: AppRoute) => void): Promise<UnlistenFn> {
  return listen<AppRoute>(APP_NAVIGATE_EVENT, (event) => onNavigate(event.payload))
}
