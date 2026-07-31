import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './',
  testMatch: '**/*.spec.ts',
  outputDir: '../test-results/e2e/results',

  reporter: [
    ['list'],
    ['html', { outputFolder: '../test-results/e2e/report', open: 'never' }],
  ],

  use: {
    baseURL: 'http://localhost:1420',
  },

  projects: [
    {
      name: 'browser-only',
      use: { ...devices['Desktop Chrome'], headless: true, mode: 'browser' },
    },
    {
      name: 'tauri',
      use: { mode: 'tauri' },
    },
  ],
});
