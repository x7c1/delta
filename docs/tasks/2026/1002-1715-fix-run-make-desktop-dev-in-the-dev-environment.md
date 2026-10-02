---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && make desktop-dev-build && grep -qaF 'io.github.x7c1.delta.dev' backend/target/debug/delta-desktop && grep -qF '\"identifier\": \"io.github.x7c1.delta\"' backend/crates/apps/delta-desktop/tauri.conf.json"
assignee: null
branch: task/1002-1715-fix-run-make-desktop-dev-in-the-dev-environment
created_at: 2026-10-02T08:15:04Z
updated_at: 2026-10-02T08:42:58Z
---

# fix(desktop): run make desktop-dev in the dev environment, not the installed app's

## Overview

There are two Delta environments a developer uses side by side:

- **The installed app** (`make desktop`, the `.deb` / `Delta.app`): identifier
  `io.github.x7c1.delta`, its database and `sessions/` under the app data
  directory, tmux socket `io.github.x7c1.delta`, the port it recorded in the hook
  state file. This is the one used day to day (dogfooding).
- **The dev environment** (`make dev`, `scripts/dev.sh`): database
  `backend/delta.db` (or `DELTA_DB_PATH`), tmux socket `io.github.x7c1.delta.dev`,
  port 7878, session working directory `.tmp/session`. `make down` and
  `make reset` act only on it.

`make desktop-dev` (`cargo run -p delta-desktop`) belongs to neither today: it is a
debug build of the installed app with the **same** identifier, so it uses the
installed app's data directory, database, tmux socket and single-instance lock.
When the installed app is running, `make desktop-dev` only focuses that window and
exits. When it is not, the development build opens the installed app's database —
running migrations that are not released yet — and re-adopts its live sessions.

Make `make desktop-dev` "the dev environment, seen through the desktop shell":

- **Identifier** `io.github.x7c1.delta.dev` for the dev build only, so its data
  directory (webview storage included) and its single-instance scope are separate
  from the installed app's. `tauri.conf.json` keeps `io.github.x7c1.delta` for
  `make desktop` and releases. Find how Tauri v2 lets a `cargo run` build take a
  different identifier (for example the `TAURI_CONFIG` environment variable that
  `tauri-build` / `generate_context!` merge, or a `tauri.dev.conf.json` merged by
  `cargo tauri dev --config …`); pick one and say why in the Makefile or guide.
- **The same database, tmux socket, port and session working directory as
  `make dev`.** The desktop shell already honours `DELTA_DB_PATH`,
  `DELTA_SESSION_WORKDIR`, `DELTA_TMUX_SOCKET` and `DELTA_PORT` (an explicit
  `DELTA_PORT` is bound as is and not recorded). Keep those values defined in one
  place — `scripts/dev.sh` owns them today — so the two cannot drift; for example
  `make desktop-dev` can go through `scripts/dev.sh` with a new mode, or both can
  read a small shared file.
- **Never both at once.** Because both bind 127.0.0.1:7878, starting one while the
  other runs must fail with a clear message instead of two servers on one database
  and socket. `scripts/dev.sh` already refuses when the port is taken; make
  `make desktop-dev` refuse the same way before launching (the app's own startup
  failure dialog is the fallback, not the intended path). `make down` stopping the
  port 7878 listener ends a running `make desktop-dev` too — that is consistent with
  "down stops the dev environment"; say so in the docs.
- **`make desktop-dev-build`**: a target that builds exactly the binary
  `make desktop-dev` runs (same profile, same identifier override) without starting
  it; `make desktop-dev` uses it and then runs the binary. The check uses it to
  prove the dev build carries the `.dev` identifier.

Docs: `docs/guides/development/README.md` (the Desktop shell section) and
`docs/guides/development/local-run.md` must describe the two environments, which
make targets belong to which, that `make dev` and `make desktop-dev` share sessions
and cannot run together, and that the port recording (hook state file) is only
exercised by the installed app because the dev environment pins 7878. Update the
`scripts/dev.sh` header if its usage changes.

Not in scope: changing the installed app's identifier, data directory or socket, and
the browser-side `make dev` behaviour beyond what sharing the values requires.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `make desktop-dev-build` builds a debug `delta-desktop` that carries the
      identifier `io.github.x7c1.delta.dev` (the `grep -qaF` gate on the binary),
      while `tauri.conf.json` still says `io.github.x7c1.delta` (the `grep -qF` gate).

### Manual / on-hardware (verified by a human before merge)

- [ ] With the installed app running, `make desktop-dev` opens its own window on the
      dev database (the sessions made with `make dev` are listed, the installed
      app's are not) and the installed app is untouched.
- [ ] With `make dev` running, `make desktop-dev` refuses with a clear message, and
      the reverse also refuses; after `make down`, either starts.
