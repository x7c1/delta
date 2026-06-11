import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for the fake-mode end-to-end suite: the real frontend
 * against the REAL backend, whose spawned "claude" is the scripted
 * `fake-claude` binary. Unlike the mock suite (`playwright.config.ts`),
 * nothing is mocked in the browser — REST, WebSocket events, and the PTY
 * bridge all hit a live `delta-server`, which spawns real tmux panes; only the
 * model behind them is a deterministic script.
 *
 * The backend is booted by `scripts/e2e-fake.sh` (the `make e2e-fake` entry
 * point), which owns the temp database, the per-run tmux socket, and teardown.
 * This config only starts the Vite dev server, proxied to that backend via
 * DELTA_PORT (see vite.config.ts).
 */

// Dedicated ports so the suite never collides with `make dev` (5173/7878) or
// the mock e2e suite (5199).
const PORT = Number(process.env.E2E_FAKE_PORT ?? 5198);
const BACKEND_PORT = Number(process.env.E2E_FAKE_BACKEND_PORT ?? 7899);

export default defineConfig({
  testDir: './e2e-fake',
  forbidOnly: !!process.env.CI,
  retries: 0,
  // One worker, no intra-file parallelism: every spec talks to the one shared
  // backend, and serial execution keeps each spec's session/tmux pane isolated
  // in time from the others.
  workers: 1,
  fullyParallel: false,
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
    env: { DELTA_PORT: String(BACKEND_PORT) },
    url: `http://localhost:${PORT}`,
    // Never adopt a stray server on the port: it could be a mock-mode one (no
    // backend behind it) and the suite would silently test the wrong thing.
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
