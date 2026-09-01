---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "rejects_a_request_without_a_token" backend/crates/apps/delta-server && grep -rq "rejects_a_websocket_upgrade_without_a_token" backend/crates/apps/delta-server && grep -rq "exempts_hooks_and_health_from_the_token" backend/crates/apps/delta-server'
assignee: null
branch: task/0901-1201-fix-per-run-bearer-token
created_at: 2026-09-01T12:01:00Z
updated_at: 2026-09-01T12:58:00Z
---

# fix(server): require a per-run bearer token on the API and live sockets

## Overview

delta-server has no authentication of any kind: every `/api/*` route and every
WebSocket upgrade (`/ws`, `/pty`, `/comms`) is served to anyone who can reach
the loopback port. The Origin/Host guard added earlier
(`backend/crates/apps/delta-server/src/origin_guard.rs`) blocks foreign web
origins, but a local non-browser process (a `curl`, another tool) sends no
`Origin` and a loopback `Host`, so the guard lets it through. Add a **per-run
bearer token** as defense in depth: the server mints (or is handed) a random
token for its lifetime, the frontend presents it on every request, and requests
without the valid token get **401**.

This is a full-stack change; the mechanism below was mapped against the code.

### Token lifecycle and delivery

1. **Mint once, outside both processes, to avoid a startup race.** In
   `scripts/dev.sh` `up()`, mint `DELTA_AUTH_TOKEN="$(openssl rand -hex 32)"`
   (or an equivalent already available on both macOS and Linux — if `openssl`
   is not guaranteed, use a portable shell mint) and export the **same** value
   into both subshells: the `cargo run -p delta-server` backend invocation and
   the `pnpm --filter @delta/web dev` (Vite) invocation.
2. **Backend reads/holds it.** In `main.rs` `config_from_env()` read
   `DELTA_AUTH_TOKEN`; if unset (bare `cargo run`), mint a random one as a
   fallback so the server always has a token. Store it as an `Arc<str>` field
   on `AppState` (`state.rs`), mirroring the existing `tmux_socket: Arc<str>`
   field; expose a `token()` accessor.
3. **Backend enforces it** with a new middleware next to `origin_guard`, wired
   in `route_binder.rs` `finish()` via
   `axum::middleware::from_fn_with_state(state.clone(), auth_guard)` (or a
   closure capturing the token). Accept the token from **either** an
   `Authorization: Bearer <token>` header (HTTP fetch) **or** a `token=<token>`
   query parameter (WebSocket upgrades — browsers cannot set headers on a WS
   handshake). The middleware sees the full request URI, so it can read the
   query param for `/ws`, `/pty`, `/comms` **without** changing those handlers'
   `Query` structs. Missing or wrong token → **401** (empty/short body). Use a
   constant-time comparison for the token check.
   - **Exempt `/hooks/*` and `/health`** by path prefix: the hooks are called
     by Claude Code (not the browser), carry no browser credentials, and are
     already loopback-guarded; `/health` is a plain probe. Exempting them means
     `delta-bootstrap/src/settings.rs` (the rendered hook URLs/curls) needs
     **no change** in this task. Document the exemption in a code comment.
   - **Middleware order:** keep the existing Origin/Host guard working and
     independently testable. Decide and document the order; the pre-existing
     `origin_guard` tests assert a 403 for a foreign origin, so they must still
     reach the origin check — update them to send a valid token (see Test blast
     radius) rather than reorder in a way that turns their 403 into a 401.
4. **Vite injects it into the page.** Add a `transformIndexHtml` plugin hook in
   `frontend/packages/apps/web/vite.config.ts` that reads
   `process.env.DELTA_AUTH_TOKEN` and injects
   `<meta name="delta-auth-token" content="…">` into `index.html`. The existing
   CSP (mirrored in `index.html` and `vite.config.ts`) permits a static meta
   tag. When the env var is unset (mock mode), inject nothing (or an empty
   value the frontend treats as "no token").
5. **Frontend presents it.**
   - Read the token once in `frontend/packages/apps/web/src/config.ts` (e.g.
     an `authToken()` helper reading the `<meta name="delta-auth-token">`).
   - Pass it into `new ApiClient({ baseUrl, token })` at
     `frontend/packages/apps/web/src/data/apiContext.tsx`, and set the
     `Authorization: Bearer <token>` header inside `ApiClient.request` /
     `requestNoContent` in
     `frontend/packages/gateway/api-client/src/http.ts` (the single fetch
     chokepoint). Add an optional `token?` to `ApiClientOptions`.
   - Append `token=<token>` inside `wsUrl()` in `config.ts` so all three
     sockets carry it. The downstream `?session_id=` joiners use
     `url.includes('?') ? '&' : '?'`, so ordering is safe.
6. **Mock mode omits it.** `make mock` / `make e2e` never reach a real backend
   (MSW intercepts; WS is a fake). When no token is present (or `isMockMode()`),
   the API client and `wsUrl()` must omit the header/param so mock mode keeps
   working. Do not require a token frontend-side; absence just means "don't
   attach".

### Test blast radius (do this up front — every router-driving test needs the token)

The middleware 401s any in-process request that lacks a valid token. Enumerate
and fix ALL of them in this PR (this mirrors the lesson from the Host-guard
change, where sibling-crate tests were missed and the check failed):

- **delta-server's own tests** under
  `backend/crates/apps/delta-server/src/app/tests/*.rs` and
  `backend/crates/apps/delta-server/tests/end_to_end.rs` — they build the router
  and `.oneshot(...)` it. They already send a loopback `host` header; they now
  also need a valid token. Prefer a **shared test helper** that reads the built
  `AppState`'s token and attaches `Authorization: Bearer <token>` (and keeps the
  loopback host), so every call site is covered in one place where a helper
  exists.
- **The `origin_guard` tests** (`app/tests/origin_guard.rs`) — the cases that
  should reach the origin check (and assert 403 / 200) must send a **valid
  token**; add a dedicated case asserting **401 when the token is missing** and
  another asserting a wrong token is 401.
- **Sibling crates** that drive `router(state)` via `.oneshot(...)`:
  `backend/crates/apps/fake-claude/tests/full_loop.rs` and
  `backend/crates/apps/fake-codex/tests/full_loop/*` (the `support.rs` helpers
  `post_json`/`get` plus the direct request sites in `interrupt.rs`,
  `session_death.rs`, `permissions/{parallel_approvals,decision_matrix,file_change_detail}.rs`).
  These build `AppState` in-process, so read `state.token()` (or set a known
  token on the `Config`) and attach it in the shared helpers.
- **e2e-fake harness:** set a fixed `DELTA_AUTH_TOKEN=<constant>` in **both**
  `frontend/packages/apps/web/playwright.fake.config.ts` (`webServer.env`, next
  to `DELTA_PORT`) **and** the backend-spawn `env` block in
  `frontend/packages/apps/web/e2e-fake/support/server.ts`, so the Vite-injected
  page and the real backend agree on the token. `e2e-real`
  (`scripts/e2e-real.sh`) follows the same pattern. The mock `make e2e`
  (`playwright.config.ts`, `VITE_API_MOCK=1`) needs no token.

If `Config` gains an `auth_token` field, update every `Config { … }`
constructor across the test harnesses so the workspace still builds.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A request to an `/api/*` route with no `Authorization` header (and a
      loopback host) is rejected with **401** (backend test name contains
      `rejects_a_request_without_a_token`; grepped by `check_command`); a
      request with a wrong token is also 401; a request with the valid token
      reaches its handler.
- [x] A WebSocket upgrade to `/ws` (and `/pty`, `/comms`) without a valid
      `token=` query param is rejected with **401** before the socket opens
      (backend test name contains `rejects_a_websocket_upgrade_without_a_token`);
      the same upgrade with the valid token is accepted.
- [x] `/hooks/*` and `/health` remain reachable **without** a token (backend
      test name contains `exempts_hooks_and_health_from_the_token`), so Claude
      Code hook callbacks and health probes are not broken.
- [x] The frontend attaches the token: `ApiClient` sends
      `Authorization: Bearer <token>` when constructed with a token and omits
      it when constructed without one (unit test in
      `frontend/packages/gateway/api-client/`), and `wsUrl()` appends
      `token=` when a token is present and omits it otherwise (unit test in
      `frontend/packages/apps/web/`).
- [x] `make check` passes green — the guard, the Vite injection, the frontend
      wiring, and every router-driving test (delta-server, end_to_end,
      fake-claude, fake-codex, origin_guard) pass under the token requirement,
      and the e2e / e2e-fake suites still pass (mock e2e needs no token;
      e2e-fake uses the fixed token).

### Manual / on-hardware (verified by a human before merge)

- [ ] In a live `make dev` session the app loads and works end to end (REST +
      live `/ws`/`/pty`/`/comms`), and the Claude Code hooks still fire — i.e.
      the token flows from `dev.sh` through Vite into the page and the backend
      accepts it. `make mock` still works with no backend. (Non-blocking for
      merge under the agreed CI-green autonomous policy; recorded for
      dogfooding.)

## Out of scope

- The per-session **hook** secret and transcript-path confinement (a separate
  change) — this task exempts `/hooks/*` from the bearer token; the hook
  endpoints get their own authentication later.
- Rotating the token during a run, or persisting it across runs — it is
  per-run, minted at startup.
- Origin/Host validation (already shipped) — this composes with it.
- Any production static-serving path (does not exist in this repo).
