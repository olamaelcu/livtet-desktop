import { svelte } from '@sveltejs/vite-plugin-svelte'
import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'
import { readFileSync } from 'node:fs'

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
  .filter(line => !line.trim().startsWith('#') && line.trim() !== '')

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
        resolve: { alias: aliases },
        test: {
          name: 'unit',
          include: ['web/lib/**/*.test.ts', 'scripts/**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        plugins: [svelte()],
        resolve: {
          alias: {
            // Component tests run without a router; stub what routes read
            // from it. The `$app` alias below points into .svelte-kit, which
            // is not generated for this project, so each used module needs its
            // own stub.
            '$app/state': new URL('./web/tests/browser/stubs/app-state.ts', import.meta.url)
              .pathname,
            '$app/navigation': new URL(
              './web/tests/browser/stubs/app-navigation.ts',
              import.meta.url,
            ).pathname,
            '$app/paths': new URL('./web/tests/browser/stubs/app-paths.ts', import.meta.url)
              .pathname,
            ...aliases,
          },
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
    ],
  },
})