import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for the headless end-to-end suite. The suite drives the web
 * app in mock mode (`VITE_API_MOCK=1`): MSW serves the REST surface from
 * fixtures and a fake event source replays a scripted `SessionEvent` sequence,
 * so the whole UI runs with no backend and is fully deterministic.
 *
 * Workspace libraries must be built first (`pnpm -r build`) so the dev server
 * can resolve them; the `webServer` below only launches Vite in mock mode.
 */

// The suite spins up its own mock-mode Vite server. `E2E_PORT` overrides the
// default so a run can avoid colliding with a real-backend dev server that may
// already hold 5173 (in which case `reuseExistingServer` would otherwise reuse
// the wrong, non-mock server).
const PORT = Number(process.env.E2E_PORT ?? 5173);

export default defineConfig({
  testDir: './e2e',
  // Deterministic mock mode: fail fast on any flake rather than masking it.
  forbidOnly: !!process.env.CI,
  retries: 0,
  fullyParallel: true,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: `http://localhost:${PORT}`,
    trace: 'on-first-retry',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: `pnpm exec vite --port ${PORT} --strictPort`,
    env: { VITE_API_MOCK: '1' },
    url: `http://localhost:${PORT}`,
    // Default to a fresh server every run so a stray process on the port is
    // never adopted — in particular a live `make dev` (real backend) on 5173,
    // which the suite cannot tell apart from its own mock build and would
    // otherwise drive, sending real prompts to `claude`. Opt back in with
    // E2E_REUSE=1 to reuse an already-running mock server for fast iteration.
    reuseExistingServer: process.env.E2E_REUSE === '1',
    timeout: 120_000,
  },
});
