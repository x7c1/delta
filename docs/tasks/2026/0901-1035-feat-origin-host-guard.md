---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'make check && grep -rq "rejects_a_cross_site_origin" backend/crates/apps/delta-server && grep -rq "rejects_a_non_loopback_host" backend/crates/apps/delta-server && grep -rq "allows_a_request_with_no_origin_header" backend/crates/apps/delta-server'
assignee: null
branch: task/0901-1035-feat-origin-host-guard
created_at: 2026-09-01T10:35:00Z
updated_at: 2026-09-01T11:05:00Z
---

# fix(server): reject cross-site Origin and non-loopback Host on every route

## Overview

The delta-server axum app applies no Origin or Host validation to any route.
The router is assembled by `RouteBinder` chaining `.bind(...)` and ending at
`.finish(state)` (`backend/crates/apps/delta-server/src/app/mod.rs:79`;
`route_binder.rs:72` is `self.router.with_state(state)`) — there is no
`.layer(...)` anywhere. The WebSocket upgrade handlers never inspect headers:
`ws.rs` (`ws_handler(State, WebSocketUpgrade)`), `pty.rs`
(`pty_handler(State, Query<PtyQuery>, WebSocketUpgrade)`), `comms.rs`
(`comms_handler(State, Query<CommsQuery>, WebSocketUpgrade)`).

Because WebSockets are **not** subject to the browser's same-origin policy or
CORS, any web page the user visits can open
`ws://127.0.0.1:7878/pty?session_id=…` and get a live read/write PTY into the
agent pane. The server binds loopback only (`main.rs:73–76`,
`Ipv4Addr::LOCALHOST`), and the port is predictable (`DELTA_PORT`, default
7878), so loopback binding alone is not a defense. Add a guard, applied to
every route, that rejects requests coming from a foreign web origin.

### The guard

Add a single middleware — a hand-rolled `axum::middleware::from_fn` (or
`from_fn_with_state`) layer — attached to the whole router where it is
finalized (`app/mod.rs` / `route_binder.rs`), so it covers `/api/*`, `/ws`,
`/pty`, `/comms`, `/hooks/*`, and `/health` alike. **Do not add `tower-http`**
— it is not a dependency (direct or transitive), and pulling it in for this is
unnecessary lockfile churn; `tower` is already available (a dev-dependency for
oneshot testing) and `axum::middleware::from_fn` needs nothing new.

Rules (return **403 Forbidden** with an empty/short body on rejection):

1. **Origin**: if the request carries an `Origin` header, its host must be a
   loopback host — `localhost`, `127.0.0.1`, or `[::1]` — on any port and with
   an `http`/`https`/`ws`/`ws s` scheme. A present `Origin` whose host is
   anything else (a real domain like `https://evil.example`) is rejected. This
   is the property that actually stops the attack: a malicious page the user
   visits is served from a real domain, so its `Origin` is not loopback.
2. **Host**: the `Host` header's host must likewise be a loopback host
   (`localhost` / `127.0.0.1` / `[::1]`, any port). A missing or non-loopback
   `Host` is rejected. This blocks DNS-rebinding, where a name that resolves to
   127.0.0.1 is used to reach the server with an attacker-controlled `Host`.
3. **Absent `Origin` is allowed** (subject to the `Host` check). This is
   load-bearing and must not regress: the Claude Code hook callbacks are
   `curl` POSTs to `/hooks/*` with **no** `Origin` header (see
   `crates/libs/delta-bootstrap/src/settings.rs`), same-origin non-browser
   clients omit it, and `/health` probes omit it. Rejecting absent-`Origin`
   would break hooks and the health check. The guard is a CSRF-style check on a
   *present* cross-site `Origin`, not an `Origin`-required check.

**Why loopback-host rather than a fixed origin allowlist.** The audit framed
the allowlist as "the served app origin + the vite dev origin". Pinning exact
origins (e.g. `http://localhost:5173`) is brittle: the app is served by Vite in
dev, the `make e2e` / `make e2e-fake` suites boot the backend and browser on
dedicated non-default ports (`DELTA_PORT` / `E2E_PORT`), and there is no
production static-serving path in this repo. The loopback-host rule blocks the
real threat (a remote web origin) without pinning a port, so it does not break
dev or the e2e suites. Document this reasoning in a code comment. (If you find
a concrete reason a fixed allowlist is required, keep it configurable via an
env var with the dev origins as the default, and make sure the e2e suites still
pass — but prefer the simpler loopback-host rule.)

Parse `Origin`/`Host` carefully: an `Origin` is a scheme + host + optional
port (no path); strip the scheme and any port before comparing the host, and
treat a malformed `Origin` as foreign (reject). Add a focused unit test for the
host-extraction helper.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A request to an `/api/*` route with `Origin: https://evil.example` is
      rejected with **403** (test name contains `rejects_a_cross_site_origin`;
      grepped by `check_command`). The same rejection holds for a
      WebSocket upgrade request to `/ws`, `/pty`, and `/comms` carrying a
      foreign `Origin` (the upgrade must be refused before the socket opens).
- [x] A request whose `Host` header is not a loopback host (e.g.
      `Host: evil.example`) is rejected with **403** (test name contains
      `rejects_a_non_loopback_host`; grepped by `check_command`).
- [x] A request with **no** `Origin` header and a loopback `Host` passes the
      guard (reaches its handler / normal status), proving hook `curl` POSTs and
      `/health` are not broken (test name contains
      `allows_a_request_with_no_origin_header`; grepped by `check_command`). A
      request with a loopback `Origin` (`http://localhost:<port>`) also passes.
- [x] `make check` passes green — the guard is wired to every route via the
      finalize/router layer, and the existing e2e / e2e-fake suites (which
      drive a real browser and the hook callbacks) still pass under it.

### Manual / on-hardware (verified by a human before merge)

- [ ] In a live `make dev` session the app, terminal, and live channels
      (`/ws`, `/pty`, `/comms`) work normally, and the Claude Code hooks fire
      (the session binds and the transcript updates). (Non-blocking for merge
      under the agreed CI-green autonomous policy; recorded for dogfooding.)

## Out of scope

- Authentication / a bearer token (a separate change): this task is Origin +
  Host validation only.
- CORS response headers: the goal is to reject forbidden requests, not to
  enable cross-origin browser access — the app is same-origin.
- Adding `tower-http`.
- Changing the loopback bind or the port-selection logic (already loopback).
