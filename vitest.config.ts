import { readFileSync } from 'node:fs'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

const aliases = {
  $lib: new URL('./web/lib', import.meta.url).pathname,
  // Route components live outside $lib, so fixtures that mount a route need
  // their own alias rather than reaching back up with ../../../.
  $routes: new URL('./web/routes', import.meta.url).pathname,
  $app: new URL('./.svelte-kit/runtime/app', import.meta.url).pathname,
}

// Read web exclusion globs from coverage-ignore.txt (single source of truth).
// Lines starting with `#` or blank are ignored. Globs are matched against
// file paths, not module specifiers — see the comment in coverage-ignore.txt.
const webExcludes = readFileSync(new URL('./coverage-ignore.txt', import.meta.url), 'utf-8')
  .split('\n')
  .filter((line) => !line.trim().startsWith('#') && line.trim() !== '')

export default defineConfig({
  resolve: { alias: aliases },
  test: {
    coverage: {
      provider: 'v8',
      reportsDirectory: 'coverage',
      reporter: ['text-summary', 'html', 'lcovonly'],
      // vitest 4 removed `coverage.all`: without `include`, only files imported
      // during the run get reported. The coverage gate's preflight (ADR 38)
      // relies on untested files still showing up, so this list is load-bearing.
      // Mirrors the $lib and $routes aliases above. Aliases do not expand in
      // glob patterns -- these are matched against file paths, not resolved as
      // module specifiers -- so the directories are spelled out. Naming
      // web/lib and web/routes separately also stops this from sweeping in
      // web/tests, which the exclude list below would otherwise have to catch.
      include: [
        'web/lib/**/*.ts',
        'web/lib/**/*.svelte',
        'web/routes/**/*.svelte',
        'scripts/**/*.mjs',
      ],
      exclude: webExcludes,
    },
    projects: [
      {
        plugins: [svelte()],
        resolve: {
          // Unit tests hit `$app/*` through modules like `searchTarget`; the
          // same router stubs as the browser/ssr projects apply.
          alias: [
            {
              find: '$app/state',
              replacement: new URL('./web/tests/browser/stubs/app-state.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/navigation',
              replacement: new URL('./web/tests/browser/stubs/app-navigation.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/paths',
              replacement: new URL('./web/tests/browser/stubs/app-paths.ts', import.meta.url)
                .pathname,
            },
            { find: '$lib', replacement: aliases.$lib },
            { find: '$routes', replacement: aliases.$routes },
          ],
        },
        test: {
          name: 'unit',
          include: ['web/lib/**/*.test.ts', 'scripts/**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        plugins: [svelte()],
        resolve: {
          // Array form: first match wins. The `$app/*` stubs must precede the
          // root `$app` mapping (a `.svelte-kit` dir that is not generated
          // for this project), which would otherwise prefix-match and shadow
          // them once Vitest 5 merges the root aliases in. Only `$app/state`,
          // `$app/navigation` and `$app/paths` are imported anywhere; the
          // bare `$app` mapping is deliberately omitted here.
          alias: [
            // Component tests run without a router; stub what routes read
            // from it.
            {
              find: '$app/state',
              replacement: new URL('./web/tests/browser/stubs/app-state.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/navigation',
              replacement: new URL('./web/tests/browser/stubs/app-navigation.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/paths',
              replacement: new URL('./web/tests/browser/stubs/app-paths.ts', import.meta.url)
                .pathname,
            },
            { find: '$lib', replacement: aliases.$lib },
            { find: '$routes', replacement: aliases.$routes },
          ],
        },
        test: {
          name: 'browser',
          include: ['web/tests/browser/**/*.browser.test.ts'],
          browser: {
            enabled: true,
            provider: playwright(),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
      {
        plugins: [svelte()],
        resolve: {
          // SSR specs render routes without a router or a browser: the same
          // `$app/*` stubs as the browser project, in the same first-match
          // order. Spec files live under `web/tests/ssr/`, which neither the
          // `unit` nor the `browser` project includes.
          alias: [
            {
              find: '$app/state',
              replacement: new URL('./web/tests/browser/stubs/app-state.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/navigation',
              replacement: new URL('./web/tests/browser/stubs/app-navigation.ts', import.meta.url)
                .pathname,
            },
            {
              find: '$app/paths',
              replacement: new URL('./web/tests/browser/stubs/app-paths.ts', import.meta.url)
                .pathname,
            },
            { find: '$lib', replacement: aliases.$lib },
            { find: '$routes', replacement: aliases.$routes },
          ],
        },
        test: {
          name: 'ssr',
          include: ['web/tests/ssr/**/*.ssr.test.ts'],
          environment: 'node',
        },
      },
    ],
  },
})
