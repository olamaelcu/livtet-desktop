// Tauri serves the app as a static SPA (adapter-static fallback) with no Node
// server, so there is nothing to render on the server. Disabling SSR app-wide
// makes dev and production behave identically and keeps server-side code paths
// away from Tauri IPC and the TanStack Query client.
export const ssr = false

export const load = () => ({ pageTitle: 'Livtet' })
