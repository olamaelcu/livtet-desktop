// Stand-in for SvelteKit's `$app/state` in component browser tests, where no
// router runs. Routes read `page.url.pathname`; tests get a fixed library URL.
// Reader routes also read `page.params.editionId`, so a fixed ULID stands in.
export const page = {
  url: new URL('/library', 'http://localhost'),
  params: { editionId: '01hzzzzzzzzzzzzzzzzzzzzzzz' },
}
