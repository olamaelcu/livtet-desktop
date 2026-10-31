// Stand-in for SvelteKit's `$app/state` in component browser tests, where no
// router runs. Routes read `page.url.pathname`; tests get a fixed library URL.
// Reader routes also read `page.params.editionId`, so a fixed ULID stands in,
// and the catalog detail route reads `page.params.catalogId`. A route that
// reads a param absent here sees undefined, which usually disables its query
// rather than failing loudly -- add the param rather than working around it.
export const page = {
  url: new URL('/library', 'http://localhost'),
  params: { editionId: '01hzzzzzzzzzzzzzzzzzzzzzzz', catalogId: 'cat-standard-ebooks' },
  // `replaceState` replaces the page state alongside the URL; routes pass the
  // current one back so that writing a query parameter does not drop it.
  state: {},
}
