import { defineConfig, devices } from '@playwright/test';
import { AUTH_TOKEN, DEV_SERVER_PORT } from './e2e-fake/support/endpoints';
import { ARTIFACT_DIR } from './e2e-fake/support/paths';

/**
 * Playwright config for the fake-mode end-to-end suite: the real frontend
 * against the REAL backend, whose spawned "claude" is the scripted
 * `fake-claude` binary. Unlike the mock suite (`playwright.config.ts`),
 * nothing is mocked in the browser — REST, WebSocket events, and the PTY
 * bridge all hit a live `delta-server`, which spawns real tmux panes; only the
 * model behind them is a deterministic script.
 *
 * Both servers — the backend and the Vite dev server proxied to it — are
 * started by a worker-scoped Playwright fixture (`e2e-fake/support/server.ts`),
 * which owns the temp database, the per-run tmux socket, the scripted-claude
 * wrapper, and teardown, can kill and relaunch the backend mid-suite for the
 * restart coverage, and checks both servers around every spec. There is no
 * `webServer` here: see that file for why the dev server is the worker's.
 * `scripts/e2e-fake.sh` (the `make e2e-fake` entry point) only builds the
 * binaries and invokes this suite.
 */

export default defineConfig({
  testDir: './e2e-fake',
  // Empties the suite's artifact dir (test-results/e2e-fake/) once per run —
  // a scope the per-worker server fixture cannot express. globalSetup.ts says
  // why the wipe cannot live in the fixture.
  globalSetup: './e2e-fake/support/globalSetup.ts',
  // The suite's own output dir, so the mock suite — which may run at the same
  // time under `make -j check` — never empties it (see playwright.config.ts).
  // It is the artifact dir itself: traces/videos/screenshots and the preserved
  // backend logs land side by side under test-results/e2e-fake/.
  outputDir: ARTIFACT_DIR,
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
    baseURL: `http://localhost:${DEV_SERVER_PORT}`,
    // Specs that read server state directly through `page.request` (see
    // support/rest.ts) bypass the app's ApiClient, which is the only thing
    // that attaches the bearer token. Set it as a default header on the whole
    // context so those direct calls carry it and are not rejected with 401
    // (the browser app's own fetches already send it and simply override this).
    extraHTTPHeaders: { authorization: `Bearer ${AUTH_TOKEN}` },
    // These failures surface only in CI and don't reproduce locally, so a
    // failing run must leave behind enough evidence to diagnose it after the
    // fact. With retries: 0 (kept deliberately, so flakes stay visible rather
    // than being masked by a passing retry), `on-first-retry` would never
    // capture anything; retain-on-failure captures the trace/video and a
    // screenshot for exactly the failing run instead. All land under the
    // suite's output dir (test-results/e2e-fake/), which CI uploads on failure.
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
});
