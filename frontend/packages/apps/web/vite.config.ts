/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The Delta server the dev server proxies to. DELTA_PORT lets a non-default
// backend be targeted — used by the fake-mode e2e suite, which boots its
// backend on a dedicated port so it never collides with `make dev`.
const backendPort = process.env.DELTA_PORT ?? '7878';

export default defineConfig({
  plugins: [react()],
  server: {
    // During real-backend dev, proxy the API and live channels to the local
    // Delta server (its default port; override with DELTA_PORT). Ignored in
    // mock mode (VITE_API_MOCK=1).
    proxy: {
      '/api': `http://127.0.0.1:${backendPort}`,
      '/ws': { target: `ws://127.0.0.1:${backendPort}`, ws: true },
      '/pty': { target: `ws://127.0.0.1:${backendPort}`, ws: true },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    // The Playwright end-to-end specs under `e2e/` and `e2e-fake/` use
    // Playwright's own runner (`pnpm e2e` / `pnpm e2e:fake`); keep them out of
    // the vitest unit-test run.
    exclude: ['**/node_modules/**', '**/dist/**', 'e2e/**', 'e2e-fake/**'],
  },
});
