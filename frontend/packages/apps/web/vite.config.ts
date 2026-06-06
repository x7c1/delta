/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    // During real-backend dev, proxy the API and live channels to the local
    // Delta server (its default port; override with DELTA_PORT). Ignored in
    // mock mode (VITE_API_MOCK=1).
    proxy: {
      '/api': 'http://127.0.0.1:7878',
      '/ws': { target: 'ws://127.0.0.1:7878', ws: true },
      '/pty': { target: 'ws://127.0.0.1:7878', ws: true },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    // The Playwright end-to-end specs under `e2e/` use Playwright's own runner
    // (`pnpm e2e`); keep them out of the vitest unit-test run.
    exclude: ['**/node_modules/**', '**/dist/**', 'e2e/**'],
  },
});
