import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './',
  testMatch: '**/*.spec.ts',
  outputDir: '../test-results/e2e',

  reporter: [
    ['list'],
    ['html', { outputFolder: '../test-results/e2e/html', open: 'never' }],
  ],

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
