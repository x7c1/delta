---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/0930-0200-feat-embed-the-built-frontend-in-delta-server
created_at: 2026-09-29T16:32:50Z
updated_at: 2026-09-29T17:14:10Z
---

# feat(server): serve the built web frontend from delta-server behind an embed feature

## Overview

delta has no production delivery path for its frontend. `make dev` runs
`delta-server` (port 7878) next to Vite's dev server (port 5173), and Vite
proxies `/api`, `/ws`, `/pty` and `/comms` to the server; `delta-server`
itself serves no static files (`backend/crates/apps/delta-server/src/app/mod.rs`
`router()` mounts only `/api/...`, `/ws`, `/pty`, `/comms`, `/hooks/...` and
`/health`). A packaged build — a desktop shell that starts the server
in-process and points a webview at it, or a plain binary someone downloads —
needs the server to hand out the built SPA itself. This task adds that, as
the first step of packaging, without touching the dev loop.

### Change

1. **Embed the SPA behind a cargo feature.** Add a feature `embed-web` to
   `delta-server` that compiles `frontend/packages/apps/web/dist/` (Vite's
   `build` output: `index.html`, `assets/*` with hashed names) into the
   binary with the `include_dir` crate (workspace dependency). Without the
   feature nothing changes: no static route, no dependency on `dist/`
   existing, so `cargo build` in a checkout that never ran the frontend
   build keeps working. With the feature, the router gains:
   - `GET /` and `GET /index.html` → `index.html`;
   - `GET /assets/<file>` and any other file present in the dir → that file,
     with a content type from its extension (`mime_guess` or a small
     extension table) and, for the hashed `assets/` files, an immutable
     long-lived `Cache-Control`; `index.html` with `no-cache`;
   - a **SPA fallback**: any other `GET` whose path does not start with
     `/api`, `/ws`, `/pty`, `/comms`, `/hooks` or `/health` and whose
     request accepts HTML → `index.html`, so deep links into the app work;
     unknown paths under the reserved prefixes keep their current 404.
   Put the static routes in their own module (`app/static_web.rs` or a
   directory module if it grows past a screen) and mount them last, after
   the API routes, so they can never shadow an endpoint. Leave
   `mockServiceWorker.js` out of the embed (it is MSW's mock-mode worker
   copied from `public/`; the packaged app never runs mock mode).
2. **Same-origin is the point.** Served from the server's own origin, the
   browser's API calls and websocket connects are same-origin, so the
   existing origin guard (`origin_guard.rs`) and the auth token
   (`DELTA_AUTH_TOKEN` / the minted fallback) must keep working exactly as
   they do through Vite's proxy today. Check how the frontend learns the
   token and the API base (`frontend/packages/apps/web/src/config*`) and
   make sure the embedded page gets both without a dev-only mechanism; if
   the dev loop passes them via Vite env, the served `index.html` needs an
   equivalent (a small JSON at a reserved path, or values injected into
   `index.html` at serve time) — choose the simplest that keeps the token
   out of the URL and out of the built assets, and document it in
   `docs/guides/api/` where the auth token is described.
3. **Make targets.** `make web-dist` runs the frontend build for the SPA
   (`pnpm --filter @delta/web build`, plus whatever workspace libraries it
   needs first, the same way `make dev` / `check-frontend-build` do).
   `make server-embedded` depends on it and runs
   `cargo build -p delta-server --features embed-web --release`. Add a
   `check-embedded` step to the `check` graph after `check-frontend-build`
   and `check-backend-build` that builds the embedded server (debug is
   fine) so the include and the routes compile against a real `dist/` on
   every gate. Add the corresponding job or step to `.github/workflows/ci.yml`
   so CI covers it too (the frontend job already builds `dist/`; the
   embedded build needs both toolchains — pick the job that has them or add
   the missing setup).
4. **Tests.** Route tests in the server crate that do not depend on the real
   `dist/`: embed a tiny fixture directory (`tests/fixtures/web-dist/` with
   an `index.html`, one `assets/app-abc123.js`, one nested file) through the
   same code path and assert: `/` and `/index.html` return the page with
   `text/html`; the asset returns with `application/javascript` and the
   immutable cache header; a deep link (`/sessions/xyz`) returns
   `index.html`; `/api/does-not-exist` and `/hooks/nope` stay 404 (or
   whatever they return today) and are not turned into HTML; a non-HTML
   `Accept` on an unknown path is not served `index.html`. Gate them with
   the feature so they run in `check-embedded` (or make the static module
   feature-independent and only the *real* dir feature-gated, so the tests
   always run — prefer this).
5. **Docs.** `docs/guides/development/README.md` (or `local-run.md`): a
   short section "Serving the built frontend" with the two make targets and
   the URL to open (`http://127.0.0.1:7878/`), and a note that `make dev`
   is unchanged. `README.md`'s Getting started stays source-only for now
   (the packaging steps that follow will rewrite it).

## Acceptance criteria

### Automated (pipeline-verified)

- [x] With the `embed-web` feature, `delta-server` serves `index.html`,
      the hashed assets with correct content types and cache headers, and
      the SPA fallback for deep links, while the reserved API/websocket/hook
      prefixes are untouched (route tests against a fixture dir).
- [x] Without the feature, the server builds and behaves exactly as before
      (no static routes; existing tests unchanged).
- [x] `make check` is green and includes `check-embedded`; CI builds the
      embedded server.

### Manual / on-hardware (verified by a human before merge)

- [ ] `make server-embedded`, run the binary with the usual env
      (`DELTA_PORT`, `DELTA_AUTH_TOKEN`), open `http://127.0.0.1:7878/` in a
      browser with no Vite running, and confirm the session list loads, a
      session's terminal attaches, and a reload on a deep link stays on that
      screen.

## Out of scope

- The desktop shell, the app data directory, the release workflow and the
  README rewrite: later steps.
- Any change to `make dev` or the Vite proxy.
