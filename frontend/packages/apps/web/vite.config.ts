/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import { bundleBudget } from './bundle-budget';

// The Delta server the dev server proxies to. DELTA_PORT lets a non-default
// backend be targeted — used by the fake-mode e2e suite, which boots its
// backend on a dedicated port so it never collides with `make dev`.
const backendPort = process.env.DELTA_PORT ?? '7878';

// Inject the per-run bearer token into the served HTML as a static
// `<meta name="delta-auth-token">` tag, read from DELTA_AUTH_TOKEN. `dev.sh`
// mints one token and exports it into both the backend and this dev server, so
// the tag the page carries matches the token the server enforces. The frontend
// reads the tag in `src/config.ts` (`authToken()`). When the env var is unset —
// notably mock mode (`make mock` / `make e2e`), which never reaches a real
// backend — nothing is injected, and the frontend treats the absent token as
// "attach nothing". A static meta tag is permitted by the existing CSP.
//
// Dev server only (`apply: 'serve'`): a production build must never bake a
// token into `dist/`, even when DELTA_AUTH_TOKEN happens to be exported. The
// built page served by delta-server (`embed-web`) gets the same tag injected by
// the server at serve time instead
// (backend/crates/apps/delta-server/src/app/static_web/index_page.rs).
function injectAuthToken(): Plugin {
  return {
    name: 'delta-inject-auth-token',
    apply: 'serve',
    transformIndexHtml() {
      const token = process.env.DELTA_AUTH_TOKEN;
      if (!token) {
        return [];
      }
      return [
        {
          tag: 'meta',
          attrs: { name: 'delta-auth-token', content: token },
          injectTo: 'head' as const,
        },
      ];
    },
  };
}

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

// Delta's bundle budget for the single JavaScript chunk, in kB. It replaces
// Vite's 500 kB default, a public-web heuristic for code sent over the network
// where first paint matters and some code may never run. Neither premise holds
// here: delta-server and the desktop app serve the SPA from 127.0.0.1 with
// immutable caching, so ~1 MB loads in milliseconds, and the terminal (xterm)
// and the Markdown renderer are used in every session, so a lazy boundary would
// only turn one request into two and add a loading state. The bundle is
// therefore deliberately not split.
//
// The budget still guards against regressions: the app is ~943 kB today, a
// dependency bump will not cross 1200 kB, pulling in one more heavy library
// will. The one constant feeds both Vite's warning limit and `bundleBudget`,
// which fails the build above it, so the two cannot drift apart.
const BUNDLE_BUDGET_KB = 1200;

export default defineConfig({
  plugins: [react(), injectAuthToken(), bundleBudget(BUNDLE_BUDGET_KB)],
  build: {
    chunkSizeWarningLimit: BUNDLE_BUDGET_KB,
  },
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
