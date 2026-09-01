/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The Delta server the dev server proxies to. DELTA_PORT lets a non-default
// backend be targeted — used by the fake-mode e2e suite, which boots its
// backend on a dedicated port so it never collides with `make dev`.
const backendPort = process.env.DELTA_PORT ?? '7878';

// Content-Security-Policy, mirrored from the <meta http-equiv> in index.html
// (keep the two in sync). The <meta> tag is the primary control that protects
// the served document; this dev response header covers the header path during
// `make dev` and, unlike a <meta> CSP, actually enforces `frame-ancestors`.
// See index.html for the per-directive rationale.
const contentSecurityPolicy =
  "default-src 'self'; base-uri 'self'; img-src 'self' data:; " +
  "font-src 'self' data:; connect-src 'self'; " +
  "script-src 'self' 'unsafe-inline' 'unsafe-eval'; " +
  "style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; object-src 'none'";

export default defineConfig({
  plugins: [react()],
  server: {
    headers: {
      'Content-Security-Policy': contentSecurityPolicy,
    },
    // During real-backend dev, proxy the API and live channels to the local
    // Delta server (its default port; override with DELTA_PORT). Ignored in
    // mock mode (VITE_API_MOCK=1).
    proxy: {
      '/api': `http://127.0.0.1:${backendPort}`,
      '/ws': { target: `ws://127.0.0.1:${backendPort}`, ws: true },
      '/pty': { target: `ws://127.0.0.1:${backendPort}`, ws: true },
      '/comms': { target: `ws://127.0.0.1:${backendPort}`, ws: true },
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    css: false,
    // The Playwright end-to-end specs under `e2e/`, `e2e-fake/`, and
    // `e2e-real/` use Playwright's own runner (`pnpm e2e` / `pnpm e2e:fake` /
    // `pnpm e2e:real`); keep them out of the vitest unit-test run.
    exclude: [
      '**/node_modules/**',
      '**/dist/**',
      'e2e/**',
      'e2e-fake/**',
      'e2e-real/**',
    ],
  },
});
