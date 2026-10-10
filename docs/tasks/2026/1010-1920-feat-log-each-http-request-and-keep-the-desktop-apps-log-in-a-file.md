---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure, error-type-design]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! grep -rnE 'uri\\(\\)\\.query|\\.query\\(\\)' backend/crates/apps/delta-server/src/request_log*"
assignee: null
branch: task/1010-1920-feat-log-each-http-request-and-keep-the-desktop-apps-log-in-a-file
created_at: 2026-10-10T10:20:06Z
updated_at: 2026-10-10T11:01:10Z
---

# feat: log each HTTP request and keep the desktop app's log in a file

## Overview

There is currently no way to see which API calls Delta's server receives.
While dogfooding the desktop app we wanted to know what runs right after
launch — the navigator mounts one `GET /api/sessions/{id}/threads` per visible
session row (`frontend/packages/apps/web/src/features/navigator/SessionNode.tsx`,
`useSessionThreadsQuery`), so the launch fans out into roughly one request per
session — and could not measure it:

- The router (`backend/crates/apps/delta-server/src/route_binder.rs`) has the
  origin and auth guards but no layer that records requests, so not a single
  line is logged per request.
- The log level is only settable through `RUST_LOG`
  (`serve::init_tracing` in `backend/crates/apps/delta-server/src/serve.rs`),
  which a desktop app started from the launcher, Dock or Finder cannot be given.
- The tracing output goes to stdout only. On Linux the desktop session's
  journal happens to keep it; on macOS an app started from Finder keeps
  nothing, so there is no log to read at all.
- The release webview has no DevTools, so the Network tab is not available
  either.

This task adds the request log and makes the desktop app's log readable and
its level switchable on both platforms.

### What to change

1. **One log line per HTTP request**, from a layer on the whole router (both
   the API routes and the static web routes, and the hook routes). Each line
   carries the method, the matched route template (axum's `MatchedPath`, e.g.
   `/api/sessions/{id}/threads`, so lines can be grouped by route), the
   request path, the response status, and the elapsed time in milliseconds.
   A WebSocket upgrade logs its upgrade like any request; the lifetime of the
   socket is out of scope.
   - Emit the lines at `debug` under a dedicated target (for example
     `delta_server::http`) so the default `info` level stays as quiet as it
     is today, and enabling only that target (`info,delta_server::http=debug`)
     turns the request log on without other debug noise.
   - **Never log the query string or any header.** The WebSocket upgrades
     carry the per-run auth token as `token=<token>` in the query
     (`auth_guard.rs`), and other routes carry filesystem paths there. Log
     `uri.path()` only. The check command greps the new module for any use of
     the query to keep this from regressing; keep the module's file name
     starting with `request_log` (a `request_log.rs` file or a
     `request_log/` directory) so the grep covers it, or update the grep in
     this file if the name has to differ.
   - A `401` from the auth guard and a `403` from the origin guard are logged
     too (the layer sits outside the guards), so a rejected call is visible.
2. **The desktop app writes its log to a file** in the platform's app log
   directory — the one Tauri's path resolver names with `app_log_dir()`,
   already listed for erasure in
   `backend/crates/apps/delta-desktop/src/erase.rs` (macOS
   `~/Library/Logs/<identifier>`, Linux `<data dir>/logs`). Keep writing to
   stdout as well, so the Linux journal and `make desktop-dev` keep working.
   - Bound the disk use: rotate (daily is fine) and keep only a small fixed
     number of files, deleting older ones. `tracing-appender` is acceptable
     as a new dependency.
   - Writing the file must not block request handling (non-blocking writer;
     keep its guard alive for the life of the process and flush on every
     exit path the app has, the same paths `window_size.rs` documents).
   - If the directory cannot be created or the file cannot be opened, log a
     `warn` to stdout and continue with stdout only — never fail the launch.
   - The `delta-server` binary keeps logging to stdout only (its dev and
     e2e wrappers already redirect it).
3. **The level is switchable without an environment variable.** `RUST_LOG`
   still wins when set. Otherwise the desktop app reads an `EnvFilter`
   directive string from a small file in its data directory (for example
   `log-filter`); a missing file means `info`; a malformed one logs a `warn`
   naming the file and falls back to `info`. It is read once at launch, so a
   change takes effect on the next launch. Decide whether the browser build's
   `delta-server` reads the same file and say so in the guide.
4. **Document it** in `docs/guides/` (the development guide, or a short
   dedicated guide linked from it): where the log file is on each OS, how to
   turn the request log on (the exact directive), and an example line. Follow
   the repository's DRY rule — `README.md` stays a command reference.

### Out of scope

- Aggregating per-route counts or timings inside the server, or showing them
  in the UI.
- Enabling DevTools in the release webview.
- Changing how many requests the navigator makes; this task only makes them
  observable.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A router-level test sends requests through the real middleware stack
      and captures tracing output (the existing `log_capture` test helper
      or an equivalent): a request to a parameterised route logs its method,
      route template, path, status and elapsed time at `debug` under the
      request-log target; a request rejected by the auth guard is logged with
      `401`; a request whose query carries `token=…` logs no part of the
      query.
- [x] At the default `info` filter no request line is emitted.
- [x] Resolving the filter is a pure function covered by unit tests:
      `RUST_LOG` set wins over the file; a missing file gives `info`; a
      valid file's directives are used; a malformed file gives `info` plus
      a warning naming the file.
- [x] Setting up the file writer is covered by a unit test against a temp
      directory: the log file is created and receives a line; an unwritable
      directory falls back to stdout only without an error.
- [x] The check command's grep finds no use of the request's query in the
      request-log module.

### Before merge (verified outside the check command)

- [ ] On this Linux machine, with the dev desktop build and the request log
      turned on through the data directory's filter file (no `RUST_LOG`),
      launching the app writes a log file under the app log directory, and
      it lists the launch's requests, including one
      `/api/sessions/{id}/threads` line per mounted session row.
- [ ] With the filter file removed, the next launch logs no request lines.
- [ ] On macOS, a build started from Finder writes the log file under
      `~/Library/Logs/<identifier>` (can be done in the Mac session that
      verifies the in-app update).
