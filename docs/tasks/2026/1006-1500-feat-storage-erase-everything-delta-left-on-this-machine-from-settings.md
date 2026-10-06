---
status: completed
pipeline_phase: null
plan: null
base_ref: null
perspectives: [completeness, clarity, user-experience]
max_refine_rounds: 3
retries_remaining: 1
check_command: "make check && git grep -q 'EraseEverything' -- backend/crates/gateway/delta-wire/src/endpoint/table.rs && git grep -q 'fn kill_server' -- backend/crates/domain/delta-usecase/src/ports/tmux_driver.rs && git grep -q 'with_graceful_shutdown' -- backend/crates/apps/delta-server/src/serve.rs && git grep -qi 'erase everything' -- docs/guides/install/README.md docs/guides/api/settings.md"
assignee: null
branch: task/1006-1500-feat-storage-erase-everything-delta-left-on-this-machine-from-settings
created_at: 2026-10-06T06:08:03Z
updated_at: 2026-10-06T07:27:32Z
---

# feat(storage): erase everything Delta left on this machine, from Settings

## Overview

Deleting `Delta.app` or running `apt remove delta-desktop` removes the
program and nothing else. Everything Delta wrote stays: the data directory
(database, hook state, session settings, tmux configuration), the tmux
server and its socket file, the worktrees under `~/.delta/worktrees`, the
`delta-<id>` branches in the user's repositories, and the trust entries
seeded into `~/.claude.json`. The install guide's "Removing everything"
walks the user through seven manual steps to find them all.

This task adds the in-app action: **Settings → Storage → Erase everything**
removes what Delta created and holds no user work, reports what it kept, and
stops the server, which then deletes its own data directory and exits. The
desktop shell's own files (webview storage) and its quitting dialog are the
follow-up task; here the desktop app only exits when its server has stopped.

### The one rule

**Delta never destroys the user's work. It removes what it created, and only
when that holds no work.** This is the rule removing a session already
follows (`delete_session`, `remove_worktree_dir`), applied once to
everything:

- a clean worktree Delta created is removed; one with uncommitted or
  untracked changes is kept;
- a `delta-<id>` branch is deleted when merged; an unmerged one, and any
  branch Delta did not create, is kept;
- everything kept is listed in the report with its reason.

Erasing offers **no `force`**. A kept worktree is removed, if the user wants
that, one at a time from the Worktrees block with its typed confirmation, or
with git. Destruction of work is always a single, explicit act.

### Server side

Add to the Interactor (`interactor/routing/`, beside `prune_sessions`) an
`erase_everything` use case that runs, in this order, and returns an
`EraseReport` (what was removed, what was kept and why — reuse `DiskItem`,
`KeepReason` and `SessionKeptItem`):

1. **Close every session**, open or still starting, through `close_session`
   (which cancels a launch in progress and stops a Codex app-server). Nothing
   is refused: there is no state in which erasing waits for the user.
2. **Kill Delta's tmux server.** Add `TmuxDriver::kill_server()` to the port;
   the gateway runs `kill-server` (a "no server running" failure is not an
   error) and then unlinks the socket file, which tmux leaves behind
   (checked on tmux 3.6a): `${TMUX_TMPDIR:-/tmp}/tmux-<uid>/<socket>`, with
   `<uid>` from `libc::getuid()` (`libc` is already a workspace dependency).
   This runs before any worktree is touched, so no `claude` process can hold
   a working directory or write a hook file into one.
3. **Remove every session** through the single removal `delete_session`, as
   `prune_sessions` does, so the worktree-and-branch rule applies unchanged
   and the kept items come back with their reasons. A session that vanished
   meanwhile is not an error.
4. **Remove the leftover worktrees** under the worktree base
   (`list_worktree_dirs`): a clean registered one through
   `remove_worktree_dir(path, false)`; a dirty or unregistered one is kept
   and reported (the existing refusals name the reason). Their branches are
   never touched, as the Worktrees block never touches them. Then remove the
   worktree base directory when it is empty, and, when the base is the
   default `~/.delta/worktrees`, `~/.delta` too when it is empty.
5. The trust entries in `~/.claude.json` go with each removed worktree, as
   today; the file itself is Claude Code's and is never deleted.

The transport (`delta-server`) then:

6. Deletes the derived files inside the data directory that the store does
   not hold open: `sessions/`, `settings/`, `tmux.conf`, the migration
   snapshots.
7. **Stops serving**: `serve` gains `axum::serve(..).with_graceful_shutdown`
   on a signal the state holds, which the erase handler triggers after its
   response is sent (graceful shutdown lets the in-flight response complete).
   `serve` returns a value saying why it stopped — add an enum such as
   `ServerStopped::{Erased(EraseReport)}` — so each shell decides what to do.
8. After the server stopped, **closes the store before deleting the database**:
   abort the background loops `serve` spawned (keep their `JoinHandle`s) and
   drop the state so the `SqliteStore` connection closes, then delete
   `delta.db`, `delta.db-wal`, `delta.db-shm`, the hook state file, and the
   data directory itself. Closing first is what matters: on Unix a file an
   open handle still writes to vanishes at exit, but a `-wal`/`-shm` file
   SQLite recreates by name would not. The hook state file is written only at
   startup (`settle_hook_secret`, `record_port`), so it needs no further guard.
9. `delta-server`'s `main` logs the report and exits 0. `delta-desktop`'s
   serve task, which today exits 1 on an error, exits 0 when the server
   stopped because of an erase (the follow-up task turns this into a dialog
   with the report and removes the shell's own files).

Endpoint: `POST /api/storage/erase` (`EraseEverything` in the wire table,
bound in `app/mod.rs`, one wire type per module under `rest/`, `make gen`
for the TypeScript bindings). It takes no body, and returns `200` with the
report:

```json
{
  "removed": {
    "sessions": 12,
    "worktrees": ["/home/u/.delta/worktrees/x7c1-delta-0198c0df-…"],
    "branches": ["delta-0198c0df-…"],
    "data_dir": "/home/u/.local/share/io.github.x7c1.delta"
  },
  "kept": [
    { "item": { "kind": "worktree", "path": "…" }, "reason": "dirty" },
    { "item": { "kind": "branch", "name": "delta-…", "repo_root": "…" }, "reason": "unmerged" }
  ]
}
```

Shape the wire `kept` entries the way `WireKeptItem` already shapes the
prune response, so the SPA renders them with the same component. A second
erase while one runs is `409` (`code: "erase_in_progress"`, an atomic flag
in the state). No preview endpoint: the Storage category already has every
input the confirmation needs (`GET /api/storage` for the paths,
`GET /api/storage/worktrees` for which worktrees have changes and will be
kept, the session list for the count).

### SPA

A final block in the Storage category (`features/settings/storage/`, a
sibling of `WorktreesBlock`), visually set apart as the destructive end of
the page:

- A heading and one paragraph: Delta will close every session, remove its
  worktrees and branches that hold no work, delete its data directory, and
  stop. What stays: worktrees with changes and unmerged branches (listed
  from the worktrees query, by directory name, before the user confirms),
  the app itself, and Claude Code's and Codex's own files under `~/.claude`
  and `~/.codex`, which Delta does not own.
- The `ConfirmPanel`, with the typed confirmation the dirty-worktree removal
  uses (the word `erase`), and the confirming button "Erase everything and
  stop Delta".
- On success, replace the whole app with a static page from the response:
  what was removed, the kept items with reasons, and "Delta has stopped.
  Close this tab." The live channels (`/ws`, `/comms`) will fail once the
  server is gone; the page must not show reconnection UI over the report.
- `api-client`: a mutation hook; `api-mocks`: an MSW handler that drains the
  mock store and answers a report; vitest for the block (typed confirmation
  gates the button; the kept list before confirming; the report page) and a
  Playwright mock spec in `e2e/settings-storage.spec.ts` for the flow.

### Docs

- `docs/guides/api/settings.md` Storage section: `POST /api/storage/erase`,
  the order above, the rule, the `409`.
- `docs/guides/install/README.md` "Removing everything": lead with the
  in-app action and what it does; keep as manual steps only what it does not
  do (remove the app package; the pre-v0.5 names; the kept worktrees and
  branches, pointing at the Worktrees block; Claude Code's and Codex's own
  files). The macOS webview storage stays a manual step until the follow-up
  task removes it from the shell.
- `docs/guides/development/local-run.md` "Shut down": one sentence that the
  erase action stops the server and deletes its data directory, and that
  `scripts/dev.sh --reset` remains the developer's quick reset.

## Acceptance criteria

### Automated (pipeline-verified)

- [x] The wire table names `EraseEverything`, the tmux port has `kill_server`,
      `serve` uses graceful shutdown, and both docs name the action (gates in
      `check_command`).
- [x] Use-case tests against the fakes fix the order (sessions closed →
      tmux server killed → session rows removed → leftover worktrees) and the
      rule: an open session is closed, not skipped; a dirty worktree and an
      unmerged branch are kept with their reasons; a clean leftover is
      removed; the empty base directory goes.
- [x] A tmux gateway test, skipped when `tmux` is absent, starts a server on
      a scratch socket, calls `kill_server` and asserts the socket file is
      gone; a second `kill_server` on the dead socket succeeds.
- [x] A server test (`app/tests/`) posts the erase against a temp data
      directory with planted files, awaits `serve` returning `Erased`, and
      asserts the directory no longer exists and the response carried the
      report; a concurrent second post is `409`.
- [x] `make check` passes, including the vitest and Playwright specs.

### Before merge (verified outside the check command)

- [x] On the development machine with a scratch `DELTA_DATA_DIR` and
      `DELTA_WORKTREE_BASE`: start `delta-server`, create one worktree
      session and leave it clean, another and leave an uncommitted file in
      it, then erase from Settings. Afterwards the data directory, the tmux
      socket file and the clean worktree are gone, `tmux -L <socket> ls`
      says no server is running, the dirty worktree is still there and was
      listed as kept, the repository has no stale worktree entry
      (`git worktree list`), and the server process exited 0.
