---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [consistency, completeness, minimalism, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: 'cd backend && cargo fmt --all -- --check && cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && test "$(grep -rl "\.route(" backend/crates/apps/delta-server/src | wc -l)" -eq 1 && BEFORE=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && make gen && AFTER=$(find frontend/packages/gateway/wire-gen/src -type f -exec shasum -a 256 {} + | sort | shasum -a 256) && [ "$BEFORE" = "$AFTER" ] && cd frontend && pnpm -r build && pnpm -r typecheck && pnpm -r test && pnpm -r lint && cd .. && make e2e'
assignee: null
branch: task/0812-0856-refactor-endpoint-declaration-table
created_at: 2026-08-12T08:56:01Z
updated_at: 2026-08-12T12:04:00Z
---

# refactor(wire): declare the endpoint table in delta-wire and build the router from it

## Overview

Today the browser↔server contract is split across two crates: `delta-wire`
owns the JSON shapes (machine-checked via ts-rs generation and the CI
gen-check), but the endpoint inventory — which paths exist, with which HTTP
methods, carrying which shapes — lives only inside axum `.route(...)` calls in
`backend/crates/apps/delta-server/src/app.rs`. Nothing machine-readable
enumerates the API surface, which is how the prose API docs drifted to
covering less than half of the real routes. The goal of this task: **reading
`delta-wire` alone tells you the entire API surface**, and delta-server
becomes a composition root that cannot mount a route the contract does not
declare (nor forget one it does).

Design (agreed):

1. **`delta-wire` gains an endpoint-declaration module.** For every route the
   server serves, a declaration carries its HTTP method, path, and — where the
   endpoint speaks JSON — its request/response wire types (referencing the
   real `Wire*` types so the association is compile-checked, not stringly).
   `delta-wire` must NOT gain an axum dependency: represent the method as a
   small local enum (`Get`, `Post`, `Patch`, `Delete` are all in use today).
   Keep a single point of declaration: the per-endpoint marker (unit struct /
   trait impl) and the crate-level `ENDPOINTS` table must be produced from one
   declaration (e.g. a `macro_rules!` that emits both), so a declaration
   cannot exist without a table row or vice versa. Channel endpoints (`/ws`,
   `/pty`, `/comms`) and the hook control plane (`/hooks/*`, payload types
   already live in `delta_wire::hooks`) are declared in the same table — the
   table is the full surface, not just `/api/*`.
2. **delta-server builds its router exclusively through a small binder.**
   axum's `Router` cannot be introspected after construction, so the binder
   records each registration as it happens: a wrapper (living in `app.rs` or a
   sibling module) exposes `bind::<E>(handler)` which matches on the declared
   method to call `get`/`post`/`patch`/`delete`, records `(method, path)`, and
   at `finish()` asserts the recorded set equals the `ENDPOINTS` table
   **exactly, in both directions** (a declared-but-unmounted endpoint and a
   mounted-but-undeclared endpoint must both panic with a message naming the
   offending route). The assert runs inside `router(state)` construction, so
   the existing unit tests in `app.rs` (which all call `router(...)`) and any
   real boot exercise it; no separate wiring is needed.
3. **Behavior is unchanged.** Same paths, same methods, same handlers, same
   responses. This is a pure re-plumbing of route registration.

The current inventory to declare (from `app.rs`; two entries share a path when
two methods are registered on it):

- `GET /health`
- `POST /hooks/{user-prompt-submit, stop, message-display, pre-tool-use,
  post-tool-use, permission-request, session-start, session-end, status-line}`
- `GET /api/sessions`, `POST /api/sessions`
- `POST /api/sessions/{id}/open`, `POST /api/sessions/{id}/close`,
  `POST /api/sessions/{id}/interrupt`
- `GET /api/sessions/{id}/threads`, `GET /api/sessions/{id}/sends`
- `GET /api/threads/{id}/messages`
- `POST /api/sends`, `POST /api/sends/{id}/cancel`,
  `POST /api/sends/{id}/release`
- `POST /api/permissions/{id}/decision`
- `POST /api/sessions/{id}/questions/{request_id}/answer`,
  `POST /api/sessions/{id}/questions/cancel`
- `GET /api/workdir/list`, `GET /api/workdir/recent`, `GET /api/workdir/git`,
  `GET /api/workdir/git/branches`
- `POST /api/open-cwd`
- `GET /api/repositories`
- `GET /api/repository-scan-roots`, `POST /api/repository-scan-roots`,
  `DELETE /api/repository-scan-roots/{path_b64}`
- `GET /api/prs`
- `GET /api/providers`
- `GET /api/launch-options`, `POST /api/launch-options`,
  `PATCH /api/launch-options/{id}`, `DELETE /api/launch-options/{id}`
- `GET /api/version`
- `GET /ws`, `GET /pty`, `GET /comms`

Note that axum merges method routers registered one at a time on the same
path (`.route(p, get(a))` then `.route(p, post(b))`), so the binder can
register one declared endpoint per call even for shared paths.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `delta-wire` contains an endpoint-declaration module in which every
      route listed in the Overview is declared with its method and path, and
      JSON endpoints additionally reference their request/response `Wire*`
      types (compile-checked type references, not string names). The
      per-endpoint declarations and the crate-level `ENDPOINTS` table are
      generated from a single declaration point (compiles under `cargo build`;
      structure reviewed against the diff).
- [x] `delta-wire` does not depend on axum (`grep -n "axum"
      backend/crates/gateway/delta-wire/Cargo.toml` returns no matches in the
      diff).
- [x] `delta-server`'s router is constructed exclusively through the binder:
      `grep -rl "\.route(" backend/crates/apps/delta-server/src` matches
      exactly one file (this gate is appended to `check_command`).
- [x] The binder's `finish()` (or equivalent) asserts mounted == declared in
      both directions, and unit tests prove both failure modes panic with a
      message naming the missing/extra route (e.g. by driving the
      record-and-assert helper directly with a truncated and an extended
      registration list).
- [x] All existing router tests in `app.rs` pass unmodified (they call
      `router(state)`, so the new assert runs in every one of them), and the
      mock e2e suite (`make e2e`) stays green.
- [x] The generated TypeScript bindings are byte-identical before and after
      (`make gen` hash comparison in `check_command`): the declaration module
      exports nothing to TS.

### Manual / on-hardware (verified by a human before merge)

- [ ] `make dev` boots the real server (the startup path constructs the
      router, so the declaration assert passes in the release binary) and the
      UI loads and can open a session.

## Out of scope

- A planned follow-up: generating or coverage-checking the prose API docs
  (`docs/guides/api/`) from the `ENDPOINTS` table. This task only creates the
  machine-readable table; docs automation builds on it later.
- Enforcing handler request/response types in the binder's signature
  (compile-time `Json<E::Response>` coupling) and modelling status codes /
  error semantics in the declarations. The declaration carries the types for
  documentation and future tooling; the binder only enforces method/path
  coverage.
- Any change to route behavior, handler logic, or the wire shapes themselves.
