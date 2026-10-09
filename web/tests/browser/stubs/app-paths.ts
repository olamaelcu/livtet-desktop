// Stand-in for SvelteKit's `$app/paths` in component browser tests. With no
// configured base path, resolving a route is the identity.
export function resolve(path: string): string {
  return path
}
