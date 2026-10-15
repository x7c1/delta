import type { Environment } from 'vitest/environments';
import { builtinEnvironments } from 'vitest/environments';

const jsdom = builtinEnvironments.jsdom;

/**
 * vitest's `jsdom` environment with Node's own `AbortController` and
 * `AbortSignal` kept on the global scope.
 *
 * jsdom replaces both globals with its own implementations but leaves
 * `Request` as Node's (undici). MSW 3 intercepts `fetch` at the socket level
 * in Node and rebuilds each request as `new Request(url, { signal })` with a
 * signal from the global `AbortController`; undici rejects a jsdom signal
 * ("Expected signal to be an instance of AbortSignal"), so every mocked
 * request fails with `fetch failed`. Keeping Node's pair makes the signal
 * and `Request` come from the same realm. The app code under test does not
 * use abort signals with DOM APIs, so it does not need jsdom's pair.
 */
const jsdomWithNodeAbort: Environment = {
  name: 'jsdom-with-node-abort',
  transformMode: jsdom.transformMode,
  async setup(global, options) {
    const { AbortController, AbortSignal } = globalThis;
    const env = await jsdom.setup(global, options);
    global.AbortController = AbortController;
    global.AbortSignal = AbortSignal;
    return env;
  },
};

export default jsdomWithNodeAbort;
