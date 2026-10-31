// Stand-in for SvelteKit's `$app/navigation` in component browser tests, where
// no router runs. `searchTarget` navigates to /library before focusing the
// search box; under test the navigation is a no-op that resolves.
export async function goto(_url: string): Promise<void> {}
// Routes that keep a query parameter in step with the screen (the settings tab)
// call `replaceState`. With no router there is no address bar to move and no
// `page.url` to update, so the URL is recorded instead: a test that asserts on
// it reads `replacedUrls`, which the route and the test reach as the same
// module. (`vi.mock('$app/navigation')` does not intercept the alias-resolved
// import, so a spy here would not see the route's call.)
export const replacedUrls: URL[] = []

export function replaceState(url: string | URL, _state: unknown): void {
  replacedUrls.push(new URL(url, 'http://localhost'))
}
