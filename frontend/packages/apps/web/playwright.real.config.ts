import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for the real-claude canary smoke: the real frontend
 * against the REAL backend, whose spawned claude is the REAL `claude` CLI.
 * Nothing in the loop is scripted — this is the lane that checks the upstream
 * contract the fake-claude suite re-enacts (`playwright.fake.config.ts`)
 * against reality. It consumes the local user's Claude subscription quota, so
 * it runs locally on demand (`make e2e-real`), never in CI.
 *
 * The backend is booted by `scripts/e2e-real.sh` (the `make e2e-real` entry
 * point), which owns the temp database, the per-run tmux socket, and
 * teardown. This config only starts the Vite dev server, proxied to that
 * backend via DELTA_PORT (see vite.config.ts).
 */

// Dedicated ports so the suite never collides with `make dev` (5173/7878),
// the mock e2e suite (5199), or the fake suite (5198/7899).
const PORT = Number(process.env.E2E_REAL_PORT ?? 5197);
const BACKEND_PORT = Number(process.env.E2E_REAL_BACKEND_PORT ?? 7897);

export default defineConfig({
  testDir: './e2e-real',
  forbidOnly: !!process.env.CI,
  // Real-claude responses are non-deterministic and the loop crosses a live
  // model; allow exactly one retry per canary for flakiness.
  retries: 1,
  // One worker, serial: every spec talks to the one shared backend and each
  // real session costs quota.
  workers: 1,
  fullyParallel: false,
  reporter: 'list',
  // A real turn includes model latency; give each spec room without letting a
  // wedged run hang forever.
  timeout: 120_000,
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
