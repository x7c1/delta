---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && grep -q 'delta-tmux-' docs/guides/install/README.md && grep -q 'Library/WebKit' docs/guides/install/README.md && grep -q 'Library/Caches' docs/guides/install/README.md && grep -q '.claude.json' docs/guides/install/README.md && grep -q 'TMPDIR' docs/guides/install/README.md && grep -q 'delta-app' docs/guides/install/README.md"
assignee: null
branch: task/1006-0340-docs-list-everything-delta-leaves-on-the-machine-in-the-install-guide
created_at: 2026-10-05T18:38:44Z
updated_at: 2026-10-05T19:40:00Z
---

# docs(install): list everything Delta leaves on the machine

## Overview

`docs/guides/install/README.md` has two sections that claim to be complete
and are not: "Where the app keeps its data" (the table and the sentence
under it) and "Removing everything". A user who follows the four removal
steps still has Delta files on the machine afterwards. This task makes
both sections match what the code actually writes, as of this tree. It
changes documentation only; no code moves.

What the code writes, with the source behind each item:

1. **The data directory** (`~/Library/Application Support/io.github.x7c1.delta/`
   on macOS, `~/.local/share/io.github.x7c1.delta/` on Ubuntu) holds
   `delta.db` together with its `delta.db-wal` and `delta.db-shm`
   (the store opens SQLite in WAL mode, `backend/crates/gateway/delta-sqlite/src/store/mod.rs`),
   the pre-migration snapshots `delta.db.bak-v<N>` (written by
   `backend/crates/gateway/delta-sqlite/src/migrations/runner.rs` before a
   destructive step and never deleted), `delta-hook-state.json`
   (`backend/crates/apps/delta-server/src/config/hook_state/mod.rs`, kept
   beside the database), and `sessions/`. The current text names only
   `delta.db`, `sessions/` and the hook state file.
2. **Webview storage.** On Ubuntu, WebKitGTK keeps it inside the data
   directory (`localstorage/`, `storage/`, `CacheStorage/`,
   `WebKitCache/`, as observed on a real install). On macOS it is
   **outside** the data directory: `~/Library/WebKit/io.github.x7c1.delta/`
   and `~/Library/Caches/io.github.x7c1.delta/`. The guide currently says
   nothing about webview storage, so step 3 of "Removing everything"
   leaves both macOS directories behind.
3. **Per-port session settings.** Every launch of a session writes
   `settings.json` under `<temp dir>/delta-<port>/`
   (`backend/crates/libs/delta-bootstrap/src/config.rs`,
   `session_settings_path`). `<temp dir>` is `std::env::temp_dir()`: `/tmp`
   on Ubuntu, but `$TMPDIR` (a per-user `/var/folders/.../T/`) on macOS.
   There is one directory per port the app has ever used, and nothing
   deletes them.
4. **The tmux configuration file.** Every tmux server Delta starts gets
   `<temp dir>/delta-tmux-<socket>.conf`, so `delta-tmux-io.github.x7c1.delta.conf`
   (`backend/crates/gateway/tmux-driver/src/tmux/mod.rs`). Never deleted.
5. **Git state in the user's repositories.** For a session that asked for
   a worktree, Delta runs `git worktree add -b delta-<session id>` under
   `~/.delta/worktrees/` (`backend/crates/gateway/git-worktree/src/git.rs`).
   Deleting `~/.delta/worktrees/` and running `git worktree prune` (step 4
   today) removes the worktree registration but leaves the `delta-<id>`
   branches in each repository. Delta also seeds a trust entry for each
   worktree path into `~/.claude.json` under `projects`
   (`backend/crates/gateway/git-worktree/src/trust.rs`); those entries
   stay too. `~/.claude.json` belongs to Claude Code, so the guide should
   tell the user the entries exist and let them decide, not instruct a
   blind delete of the file.
6. **Names from before the rename.** Installs from before the app was
   renamed to `delta-desktop` with identifier `io.github.x7c1.delta` (the
   v0.4 line) left `~/Library/WebKit/delta-app/` and
   `~/Library/Caches/delta-app/` on macOS, a tmux socket named `delta`
   and its `delta-tmux-delta.conf`. The current code never touches these
   names. One sentence in "Removing everything" is enough: if you ran a
   version before v0.5.0, these may also exist.

Rewrite the two sections around those facts:

- "Where the app keeps its data": keep the platform table, then list what
  the data directory holds (item 1), then say where the webview storage
  is on each platform (item 2), then the two temp-directory files (items
  3 and 4) with the platform-specific temp dir named explicitly. Keep the
  existing sentence that transcripts stay where Claude Code and Codex
  write them. Keep "The hook state file" as it is.
- "Removing everything": one ordered procedure that, followed on either
  platform, leaves nothing from the list above except what the user
  chooses to keep. Keep the current order (quit and `kill-server` first,
  then remove the app, then delete data), add the webview directories,
  the temp-directory entries, the branches and the `~/.claude.json`
  entries, and the pre-rename note (item 6). Do not restate the hook
  state explanation here; link the section.
- Do not describe any planned relocation of these files. The guide
  documents the tree it ships with.

Keep the guide's existing voice (second person, short sentences, platform
names "macOS" and "Ubuntu") and keep the Overview section at the top of
the file intact, since the file is over 100 lines.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] `docs/guides/install/README.md` names the tmux configuration file
      pattern `delta-tmux-`, the macOS webview directories under
      `Library/WebKit` and `Library/Caches`, the `~/.claude.json` trust
      entries, the macOS temp directory via `TMPDIR`, and the pre-rename
      `delta-app` directories (each is a `grep -q` appended to
      `check_command`; all six fail on the current tree).

### Manual / on-hardware (verified by a human before merge)

- [ ] On a macOS install that has run at least one session, following
      "Removing everything" leaves no `io.github.x7c1.delta` entry under
      `~/Library/Application Support`, `~/Library/WebKit`,
      `~/Library/Caches`, `$TMPDIR`, or `/tmp/tmux-<uid>/`.
- [ ] On an Ubuntu install, the same procedure leaves no
      `io.github.x7c1.delta` entry under `~/.local/share`, `/tmp`, or
      `/tmp/tmux-<uid>/`.
