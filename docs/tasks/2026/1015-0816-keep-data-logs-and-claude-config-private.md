---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check"
assignee: null
branch: task/1015-0816-keep-data-logs-and-claude-config-private
created_at: 2026-10-14T23:16:34Z
updated_at: 2026-10-15T00:09:27Z
---

# fix(security): keep Delta's data, logs and Claude Code's config private to the user, and keep prompt text out of default logs

## Overview

Several files Delta creates or rewrites end up readable by other local users,
and two log lines record whole prompts at the default level.

### File modes

1. **Data directory on Linux.** In the desktop shell the log directory
   (`~/.local/share/<id>/logs`, `apps/delta-desktop/src/logging/app_log_dir.rs`)
   and Tauri's webview data directory are created inside the data directory
   before the server's `create_dirs` runs
   (`libs/delta-bootstrap/src/data_layout.rs`), so `<id>/` is created with the
   umask default and `create_dirs` leaves an existing directory's mode alone.
   `delta.db`, its `-wal`/`-shm`, the migration snapshots (`.bak-v<N>`,
   `gateway/delta-sqlite/src/migrations/runner.rs`), the logs and `updates/`
   inherit that. Fix:
   - create the data directory with mode `0700` at the very start of the
     shell's `main`, before logging and Tauri start;
   - make `create_dirs` tighten an existing default data directory, its
     `logs/` and `updates/` to `0700` (only directories Delta owns and
     created by name, never a user-supplied `DELTA_DATA_DIR`'s parent);
   - after opening the database and after writing a snapshot, set `0600` on
     the database file, `-wal`, `-shm` and the snapshot (tightening existing
     files too);
   - create log files `0600`.
   macOS behaves the same way after the change (its `~/Library` is already
   private, so nothing visible changes there).
2. **`~/.claude.json`.** `write_atomic` in
   `gateway/git-worktree/src/trust.rs` writes a temp file with the umask
   default and renames it over the original, so every trust update loosens
   the file to `0644`. Create the temp file with `create_new` and
   `O_NOFOLLOW`, with the original file's mode (or `0600` when there is no
   original), then rename. Use an unpredictable temp name in the same
   directory.
3. Correct the comment in `gateway/workspace-fs/src/workspace/create_dir.rs`
   that says the mode is not affected by the umask.

### Logs

4. `domain/delta-usecase/src/interactor/hooks/on_user_prompt_submit.rs` (info,
   `expected`/`got`) and `domain/delta-usecase/src/interactor/sync/sync_transcript.rs`
   (warn, `sent`/`recorded`) log whole prompt texts when Claude Code rewrites
   a prompt. Log the lengths and a short hash at those levels instead, and the
   texts only at `debug`.
5. `docs/guides/development/logging.md`: say that `trace` records terminal
   input and output and live-channel payloads (the WebSocket library logs
   frame contents at trace), so a trace log must not be shared as is. When the
   resolved filter enables `trace` for everything, log one `warn` line saying
   so at startup.
6. `libs/delta-bootstrap/src/config.rs`: wrap the auth token and the hook
   secret in a newtype whose `Debug` prints `"***"`, so a future `?config`
   cannot print them.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] A test creates a data directory that already exists with `0755`
      (and `logs/`, `updates/` inside) and asserts `create_dirs` leaves them
      `0700`; a fresh one is created `0700`.
- [x] A test opens a database in a temp data directory and asserts the
      database file, `-wal` and `-shm` are `0600`; a migration snapshot is
      `0600`; a pre-existing `0644` database is tightened to `0600`.
- [x] A test asserts `ensure_dir_trusted` keeps an existing `0600`
      `~/.claude.json` at `0600`, creates a missing one as `0600`, and does
      not follow a symlink planted at the temp path.
- [x] Tests assert the two prompt-mismatch log lines at info/warn carry no
      prompt text (only lengths and a hash).
- [x] A test asserts `format!("{:?}", config)` contains neither the auth token
      nor the hook secret.
- [x] A test asserts the startup warning is logged when the filter enables
      `trace` globally and not otherwise.
- [x] `make check` passes.

### Before merge (verified outside the check command)

- [ ] macOS desktop (`make desktop-dev`): after start, the data directory,
      `delta.db` and the log file have modes `0700`/`0600`/`0600` (`stat`).
