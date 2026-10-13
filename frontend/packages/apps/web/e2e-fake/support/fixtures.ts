import { test as base } from '@playwright/test';
import { bootServer, type ServerHandle } from './server';

/**
 * The fake-mode suite's Playwright fixtures.
 *
 * Every spec imports `test`/`expect` from here instead of `@playwright/test`
 * so that a worker-scoped fixture owns the lifecycle of `delta-server` and the
 * Vite dev server (see `support/server.ts`).
 *
 * Why a worker fixture and not `globalSetup`: `globalSetup` runs in a separate
 * process and cannot hand a live child-process handle to specs, so a spec
 * could never kill and relaunch the server. A worker-scoped fixture holds the
 * handle in the worker process itself; because the suite runs `workers: 1` /
 * `fullyParallel: false` (see `playwright.fake.config.ts`), there is exactly
 * one server for the whole serial suite, and a worker crash automatically
 * re-runs the fixture and reboots it. Emptying the artifact dir happens once
 * per run instead, in `support/globalSetup.ts`.
 *
 * The `server` fixture is `auto`, so it boots for every spec even the ones
 * that never name it; the restart spec additionally declares `server` in its
 * args to reach {@link ServerHandle.restart}.
 */
export const test = base.extend<
  { serverHealth: void },
  { server: ServerHandle }
>({
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
  // Both servers' health, checked around every spec so a dead one is charged
  // to the spec during which it died. Before the spec, whatever is down is
  // brought back (a server that died between specs must not fail the next one
  // with a refused connection). After it, a server still down fails that spec
  // — Playwright then replaces the worker, so the next spec boots fresh ones.
  serverHealth: [
    async ({ server }, use, testInfo) => {
      const restored = await server.restoreDown();
      if (restored.length > 0) {
        testInfo.annotations.push({
          type: 'restored',
          description: `${restored.join(' and ')} found down before this spec, restarted`,
        });
      }
      await use();
      const down = await server.findDown();
      if (down.length > 0) {
        throw new Error(
          `The ${down.join(' and ')} went down during this spec and stayed ` +
            `down. Later specs get a fresh worker; the server logs are under ` +
            `the boot directories in test-results/e2e-fake/.`,
        );
      }
    },
    { auto: true },
  ],
});

export { expect, type Page } from '@playwright/test';
