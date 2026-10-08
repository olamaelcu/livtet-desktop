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
      include: ['web/lib/**/*.ts', 'web/**/*.svelte'],
      exclude: ['web/lib/bindings.ts', 'web/tests/**', '**/*.test.ts'],
    },
    projects: [
      {
        plugins: [svelte()],
        resolve: { alias: aliases },
        test: {
          name: 'unit',
          include: ['web/lib/**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        plugins: [svelte()],
        resolve: {
          alias: {
            // Component tests run without a router; stub the page store.
            '$app/state': new URL('./web/tests/browser/stubs/app-state.ts', import.meta.url)
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
