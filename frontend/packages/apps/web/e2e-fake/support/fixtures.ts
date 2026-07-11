import { test as base } from '@playwright/test';
import { bootServer, type ServerHandle } from './server';

/**
 * The fake-mode suite's Playwright fixtures.
 *
 * Every spec imports `test`/`expect` from here instead of `@playwright/test`
 * so that a worker-scoped fixture owns the `delta-server` lifecycle (see
 * `support/server.ts`) — the ownership `scripts/e2e-fake.sh` used to hold.
 *
 * Why a worker fixture and not `globalSetup`: `globalSetup` runs in a separate
 * process and cannot hand a live child-process handle to specs, so a spec
 * could never kill and relaunch the server. A worker-scoped fixture holds the
 * handle in the worker process itself; because the suite runs `workers: 1` /
 * `fullyParallel: false` (see `playwright.fake.config.ts`), there is exactly
 * one server for the whole serial suite, and a worker crash automatically
 * re-runs the fixture and reboots it.
 *
 * The `server` fixture is `auto`, so it boots for every spec even the ones
 * that never name it; the restart spec additionally declares `server` in its
 * args to reach {@link ServerHandle.restart}.
 */
export const test = base.extend<Record<string, never>, { server: ServerHandle }>({
  server: [
    // Playwright requires the fixtures argument to be a destructuring pattern;
    // this worker fixture depends on none, hence the empty pattern.
    // eslint-disable-next-line no-empty-pattern
    async ({}, use) => {
      const handle = await bootServer();
      await use(handle);
      await handle.teardown();
    },
    { scope: 'worker', auto: true },
  ],
});

export { expect, type Page } from '@playwright/test';
