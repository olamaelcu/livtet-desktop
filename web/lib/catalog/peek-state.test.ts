// peek-state.svelte.ts uses $state runes at module scope. In Vitest's
// node environment, $state is not transformed unless the file is compiled
// with the Svelte Vite plugin. These tests verify the JavaScript semantics
// of the module by importing it in a DOM environment (browser project).
//
// For the node project, we test the module via the search module's indirect
// usage, or we can test it in browser mode.
//
// Since peek-state is a tiny module-scoped $state reactive, testing its
// logic in isolation requires Svelte compiler support. The search tests
// and browser tests cover the actual component state.
