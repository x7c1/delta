import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config for the fake-mode end-to-end suite: the real frontend
 * against the REAL backend, whose spawned "claude" is the scripted
 * `fake-claude` binary. Unlike the mock suite (`playwright.config.ts`),
 * nothing is mocked in the browser — REST, WebSocket events, and the PTY
 * bridge all hit a live `delta-server`, which spawns real tmux panes; only the
 * model behind them is a deterministic script.
 *
 * The backend is booted by a worker-scoped Playwright fixture
 * (`e2e-fake/support/server.ts`), which owns the temp database, the per-run
 * tmux socket, the scripted-claude wrapper, and teardown — and can kill and
 * relaunch the server mid-suite for the restart coverage. `scripts/e2e-fake.sh`
 * (the `make e2e-fake` entry point) only builds the binaries and invokes this
 * suite. This config starts the Vite dev server, proxied to the backend via
 * DELTA_PORT (see vite.config.ts); the fixture spawns the server on that same
 * `E2E_FAKE_BACKEND_PORT`.
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
  // Every observable in this suite sits at the end of a real multi-hop loop
  // (composer POST → keystrokes → tmux pane → the scripted fake → the JSONL
  // transcript tail → WS broadcast → render), so even a fresh session's first
  // turn legitimately takes seconds on a loaded CI runner. Playwright's 5s
  // default expect timeout is calibrated for in-browser UI, not that loop, and
  // intermittently failed honest first-turn waits (the `toHaveCount(2)` right
  // after `startNewSession`, present in most specs). Give every expectation
  // the generosity the long cross-turn waits already carry explicitly: a
  // passing assertion still resolves the moment its condition holds, so only
  // genuine failures report slower.
  expect: { timeout: 15_000 },
  use: {
    baseURL: `http://localhost:${PORT}`,
    // These failures surface only in CI and don't reproduce locally, so a
    // failing run must leave behind enough evidence to diagnose it after the
    // fact. With retries: 0 (kept deliberately, so flakes stay visible rather
    // than being masked by a passing retry), `on-first-retry` would never
    // capture anything; retain-on-failure captures the trace/video and a
    // screenshot for exactly the failing run instead. All land under the
    // suite's output dir (test-results/), which CI uploads on failure.
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
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
    // Locally, adopt a server already bound to the port: a hard-killed run
    // (Ctrl-C storm, kill -9) leaks its Vite child, and with strict mode the
    // next run would abort on this port check before the worker fixture's
    // stale-run sweep ever gets to run. The port is dedicated to this suite,
    // so a squatter is that leaked Vite — same config, serving current
    // sources on demand — and adopting it is what lets an interrupted run
    // self-heal. CI keeps strict mode: its runners are fresh, so a bound
    // port there is a real error that must stay loud.
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
