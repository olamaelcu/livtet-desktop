// Stand-in for SvelteKit's `$app/navigation` in component browser tests, where
// no router runs. `searchTarget` navigates to /library before focusing the
// search box; under test the navigation is a no-op that resolves.
export async function goto(_url: string): Promise<void> {}
