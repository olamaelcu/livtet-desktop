// Stand-in for SvelteKit's `$app/state` in component browser tests, where no
// router runs. Routes read `page.url.pathname`; tests get a fixed library URL.
export const page = { url: new URL('/library', 'http://localhost') }
