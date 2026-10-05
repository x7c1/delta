---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, rust-module-structure]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && ! git grep -nE 'DELTA_DB_PATH|DELTA_SESSION_WORKDIR' -- . ':!docs/tasks' && test ! -e backend/crates/apps/delta-desktop/src/app_data && ! git grep -n 'temp_dir()' -- backend/crates/libs/delta-bootstrap/src/config.rs backend/crates/gateway/tmux-driver/src && git grep -q 'DELTA_DATA_DIR' -- docs/guides/development/README.md && git grep -q 'DELTA_IDENTIFIER' -- docs/guides/development/README.md"
assignee: null
branch: task/1006-0425-refactor-config-one-data-directory-derived-from-the-identifier
created_at: 2026-10-05T19:23:42Z
updated_at: 2026-10-05T20:16:45Z
---

# refactor(config): give the server one data directory derived from its identifier

## Overview

The server owns Delta's on-disk state but does not know where it is. Each
location is a separate `Config` field with its own default and its own
environment override (`backend/crates/apps/delta-server/src/config/mod.rs`,
`config_from_vars`): the database defaults to `delta.db` in the current
directory, the per-spawn working directories to `.tmp/session`, and the
desktop shell then moves those two under Tauri's app data directory
(`backend/crates/apps/delta-desktop/src/app_data/placement.rs`,
`explicit_paths.rs`) while `scripts/dev.sh` passes yet another set. Two
files escape every shell: the per-port session settings
(`delta_bootstrap::Config::session_settings_path`, `<temp dir>/delta-<port>/settings.json`)
and the tmux configuration (`backend/crates/gateway/tmux-driver/src/tmux/mod.rs`,
`<temp dir>/delta-tmux-<socket>.conf`). Nothing can list what Delta has
written, so the install guide's inventory drifted and the upcoming storage
view, cleanup and erase features would each have to enumerate the same
paths again.

This task gives the server one **data directory** and one **identifier**,
derives every path it owns from them in one place, and reduces each shell
to choosing the identifier.

### Model

- `Config` gains `identifier: String` (default `io.github.x7c1.delta`,
  override `DELTA_IDENTIFIER`) and `data_dir: String` (override
  `DELTA_DATA_DIR`; default `<platform data dir>/<identifier>`, where the
  platform data dir is `~/Library/Application Support` on macOS and
  `$XDG_DATA_HOME` or `~/.local/share` on Linux — the same resolution Tauri's
  `app_data_dir()` uses, so existing desktop installs keep their directory.
  Use the `dirs` crate, already in `Cargo.lock` through Tauri, or an
  equivalent already-vendored resolution; do not hand-roll `$HOME` joins
  for both platforms).
- Derived from `data_dir`, in **one** function (a `DataLayout`-style value
  built from `Config`, or methods on `Config`; pick one and say why in the
  PR): the database `delta.db` (with its `-wal`/`-shm` and `.bak-v<N>`
  siblings, which SQLite and the migration runner already place beside
  it), the hook state file `delta-hook-state.json` (already "beside the
  database"; it now lands in `data_dir` by construction), the per-spawn
  working-directory base `sessions/`, the session settings file
  `settings/<port>.json`, and the tmux configuration file `tmux.conf`.
  Remove `DEFAULT_DATABASE_PATH`, `DEFAULT_SESSION_WORKDIR`, the
  `DELTA_DB_PATH` and `DELTA_SESSION_WORKDIR` overrides, and
  `session_settings_path`'s use of `std::env::temp_dir()`.
- The tmux socket name defaults to the identifier (today the constant
  `DEFAULT_TMUX_SOCKET` happens to equal it). `DELTA_TMUX_SOCKET` stays as
  an override because the test harnesses give every run a pid-suffixed
  socket. `Tmux::new` takes the configuration file path from the layout
  instead of computing one under `temp_dir()`.
- `worktree_base` keeps its default (`$HOME/.delta/worktrees`) and its
  `DELTA_WORKTREE_BASE` override: worktrees are user-visible paths that git
  metadata and Claude Code's trust configuration record, so they stay
  outside the data directory on purpose. Nothing else about worktrees
  changes.
- The server creates `data_dir` and `sessions/` itself at startup
  (bootstrap or `config_from_env`, before the database opens), so the
  desktop shell no longer creates directories. Create `data_dir` with mode
  `0700`: the settings file embeds the hook secret, and the owner-only
  directory is what `workspace-fs/src/workspace/settings.rs` currently
  builds by hand under the world-writable temp directory. Keep that
  module's refusal to follow symlinks; drop only what the temp-directory
  placement made necessary and update its doc comment to the new
  location.
- **Create the per-spawn directory before the session starts.** For a
  session launched without a repository, `workdir_for` computes
  `sessions/<token>` (`backend/crates/domain/delta-usecase/src/interactor/lifecycle/workdir_for.rs`)
  but nothing creates it — `ensure_dir_trusted` only writes the trust entry,
  and tmux silently ignores a `-c` directory that does not exist, so the
  agent starts in the tmux server's own working directory. Add a
  `Workspace` port method that creates a directory (owner-only) and call it
  from the launch path (`launch_prep.rs`, and the adapter-session path in
  `adapter_session/spawn_adapter_session.rs` if it computes the same
  path) before the pane is created. Cover it with a usecase test against
  the fake workspace and a `workspace-fs` test.

### Shells

- **Desktop** (`backend/crates/apps/delta-desktop/src/main.rs`,
  `start_server`): pass `context.config().identifier` into the config
  (an explicit constructor argument or `DELTA_IDENTIFIER` set before
  `config_from_env`; prefer the argument, since the process environment is
  what the startup-order work is moving away from) and delete the
  `app_data` module. The dev build's identifier `io.github.x7c1.delta.dev`
  then yields its own data directory and tmux socket with no further
  settings.
- **`scripts/dev.sh`**: set `DELTA_IDENTIFIER` to `$DEV_NAME` and stop
  passing `DELTA_DB_PATH` and `DELTA_SESSION_WORKDIR`; derive the tmux
  socket the same way for `make down`. `--reset` deletes the database
  files inside the dev data directory instead of `backend/delta.db`.
  Developers who have a `backend/delta.db` from before keep it as an
  orphan; say so in the "Changes" of the PR and in `local-run.md`, do not
  migrate it.
- **Test harnesses** point `DELTA_DATA_DIR` at their run directory in
  place of `DELTA_DB_PATH`/`DELTA_SESSION_WORKDIR`:
  `frontend/packages/apps/web/e2e-fake/support/server.ts`,
  `scripts/e2e-real-claude.sh`,
  `backend/crates/apps/fake-claude/tests/full_loop.rs`,
  `backend/crates/apps/delta-server/tests/real_claude_canary.rs`, and the
  `config_from_vars` closures in `delta-server` tests (`serve.rs` tests,
  `config/tests.rs`). After this change a test run leaves nothing under the
  temp directory except what its run directory holds.

### Documentation

- `docs/guides/development/README.md`: the environment-variable table
  gains `DELTA_IDENTIFIER` and `DELTA_DATA_DIR` and loses the two removed
  variables; state the derived layout once.
- `docs/guides/development/local-run.md`: the dev-mode paths, the
  `make desktop-dev` paragraph, and the "Database" row of the comparison
  table follow the new model.
- `docs/guides/install/README.md`, "Where the app keeps its data": the
  settings file and the tmux configuration now live in the data
  directory; remove them from the temp-directory list, and keep the
  removal procedure's temp-directory step as a note for installs that ran
  a version before this change (they still have `<temp dir>/delta-<port>/`
  and `delta-tmux-*.conf`).

Keep the Overview section at the top of every guide over 100 lines.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `DELTA_DB_PATH` and `DELTA_SESSION_WORKDIR` no longer appear anywhere
      outside `docs/tasks/` (`git grep` gate in `check_command`), and the
      `backend/crates/apps/delta-desktop/src/app_data` module is gone
      (`test ! -e` gate).
- [x] Neither `delta-bootstrap/src/config.rs` nor `tmux-driver/src` calls
      `std::env::temp_dir()` (`git grep` gate): the settings file and the
      tmux configuration are derived from the data directory.
- [x] A unit test builds the layout from a `Config` with a given
      `data_dir` and asserts every derived path (database, hook state,
      `sessions/`, `settings/<port>.json`, `tmux.conf`) sits under it; a
      second test asserts the default `data_dir` ends with the identifier
      and the default tmux socket equals the identifier.
- [x] A usecase test shows a launch without a repository creates the
      per-spawn directory through the `Workspace` port before the pane is
      created, and a `workspace-fs` test shows the directory is created
      owner-only.
- [x] `docs/guides/development/README.md` documents `DELTA_IDENTIFIER` and
      `DELTA_DATA_DIR` (`git grep` gates).
- [x] `make check` passes, including `check-desktop-build` (the desktop
      crate compiles without the `app_data` module) and `check-e2e-fake`
      (the harness boots the server with `DELTA_DATA_DIR`).

### Manual / on-hardware (verified by a human before merge)

- [ ] On macOS and Ubuntu, an existing desktop install opens its previous
      database (same directory as before) and, after launching one
      session, has `settings/<port>.json` and `tmux.conf` inside the data
      directory and nothing new under the temp directory.
- [ ] `make dev` writes to `~/.local/share/io.github.x7c1.delta.dev/` (or
      the macOS equivalent), and a session launched without a repository
      runs its agent inside `sessions/<token>` of that directory.
