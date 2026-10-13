/**
 * Where the fake-mode suite's two servers listen, and the bearer token they
 * share — read by both `playwright.fake.config.ts` (the page's base URL and
 * request headers) and the server fixture (`server.ts`, which starts both
 * servers), so each value is spelled once.
 *
 * Dedicated ports so the suite never collides with `make dev` (5173/7878), the
 * mock e2e suite (5199) or the real-claude canary (5197/7897).
 */

/** The Vite dev server the page loads, proxied to the backend. */
export const DEV_SERVER_PORT = Number(process.env.E2E_FAKE_PORT ?? 5198);

/** The backend (`delta-server`) the dev server proxies to. */
export const BACKEND_PORT = Number(process.env.E2E_FAKE_BACKEND_PORT ?? 7899);

/**
 * A fixed per-run bearer token. The dev server injects it into the page as the
 * `delta-auth-token` meta tag, every backend generation is started with it, and
 * the config sends it on the specs' direct REST calls — so the page's token
 * always matches the one the backend enforces.
 */
export const AUTH_TOKEN = 'delta-e2e-fake-auth-token';
