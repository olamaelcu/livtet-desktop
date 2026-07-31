import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['web/**/*.{test,spec}.ts'],
    environment: 'node',
  },
});
