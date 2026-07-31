// catalog-match-state.svelte.ts uses $state runes at module scope.
// In node mode, $state is not compiled. Testing this module requires
// the Svelte compiler plugin (browser project). See
// catalog-match-state.browser.test.ts for browser-mode tests.
