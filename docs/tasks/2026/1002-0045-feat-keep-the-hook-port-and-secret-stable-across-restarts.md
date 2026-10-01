---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, error-type-design, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1002-0045-feat-keep-the-hook-port-and-secret-stable-across-restarts
created_at: 2026-10-01T15:30:04Z
updated_at: 2026-10-01T15:54:47Z
---

# feat(server): keep the hook port and secret stable across restarts

## Overview

Claude Code sessions run in tmux and outlive the Delta process: quitting the desktop
app, a crash, or an upgrade leaves them running. Each session reaches Delta through
hook URLs rendered into the settings file it was launched with
(`claude --settings <file>`), and a running Claude Code keeps using those URLs until
it exits. Both halves of the URL change on every Delta start today:

- the port — the desktop app binds `127.0.0.1:0` for a fresh port each launch
  (`backend/crates/apps/delta-app/src/main.rs`, `port_from_env().unwrap_or(0)`);
  `make dev` pins 7878 through `DELTA_PORT`, so only the app is affected;
- the hook secret — minted per run unless `DELTA_HOOK_SECRET` is set
  (`backend/crates/apps/delta-server/src/config/secrets.rs`, `hook_secret`), for the
  app and `make dev` alike.

So after a restart a surviving session's hooks go to a dead port, or reach Delta with a
secret it refuses. This task makes both stable so a later change can re-adopt those
sessions. It does not re-adopt anything itself.

What to build:

- **Persist the hook secret** in a small state file stored next to the database
  (the directory of `database_path`, so the app's data directory and the dev
  database's directory each get their own). Read it on start; mint and write it when
  it is missing. `DELTA_HOOK_SECRET` still wins when set. The file must be created
  owner-only (0600); an existing file with looser permissions is tightened (or the
  start refuses with a clear error — pick one and document it). Deleting the file
  rotates the secret on the next start.
- **Persist the port for the desktop app.** Store the port the app bound in the same
  state file; on the next launch try that port first and fall back to `127.0.0.1:0`
  when it is taken. An explicit `DELTA_PORT` still wins and is not persisted over.
  When the fallback happens, log it at `warn` and expose the fact to the rest of the
  server (for example a flag on the config/state such as "hook endpoint changed since
  the previous run") so the follow-up re-adoption change can tell the user that
  surviving sessions cannot reach Delta. The CLI server (`make dev`, bare
  `delta-server`) keeps its fixed default port and only gains the persisted secret.
- **Settings file path.** Session settings are written to
  `$TMPDIR/delta-<port>/settings.json` (`backend/crates/libs/delta-bootstrap/src/config.rs`,
  `session_settings_path`). With a stable port and secret the path and contents stay
  the same across restarts; keep it that way and make sure a restart rewrites it with
  the same values rather than leaving a stale one.
- Keep the auth token (`DELTA_AUTH_TOKEN`, the browser's bearer token) per run: it is
  not part of any hook URL and does not need to survive.

Document the state file (location, what it holds, how to rotate) in the install guide
(`docs/guides/install/README.md`) and the local-run guide.

Out of scope: discovering or re-binding surviving tmux sessions, and any UI. Those
belong to the follow-up change.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test shows the hook secret is read back from the state file on a second
      start, minted and written when the file is absent, and overridden by
      `DELTA_HOOK_SECRET`.
- [x] A test shows the state file is created with mode 0600, and that a file with
      looser permissions is handled as documented.
- [x] A test shows the app's port selection prefers the persisted port, falls back
      to an ephemeral port when it is taken (and reports that it changed), and
      leaves an explicit `DELTA_PORT` untouched.

### Manual / on-hardware (verified by a human before merge)

- [ ] Launch the desktop app, start a session, quit and relaunch: the app comes up
      on the same port, and the session's `settings.json` is unchanged.
