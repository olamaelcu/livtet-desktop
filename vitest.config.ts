import { svelte } from '@sveltejs/vite-plugin-svelte'
import { playwright } from '@vitest/browser-playwright'
import { defineConfig } from 'vitest/config'

const aliases = {
  $lib: new URL('./web/lib', import.meta.url).pathname,
  $app: new URL('./.svelte-kit/runtime/app', import.meta.url).pathname,
}

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
      include: ['web/lib/**/*.ts', 'web/**/*.svelte', 'scripts/**/*.mjs'],
      exclude: [
        'web/lib/bindings.ts',
        'web/tests/**',
        '**/*.test.ts',
        // Build staging: side-effect wiring around pdfjs-dist, not logic.
        'scripts/stage-pdfjs.mjs',
      ],
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
