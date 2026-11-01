/**
 * Whether the webview is running on macOS.
 *
 * Kept in one place so the chrome that reserves space for the traffic lights
 * and the settings that only exist on macOS agree on the platform.
 */
export const isMacOs = /Mac/.test(navigator.platform)
